// The bar is where the app-wide background checks live: the PR badge and,
// beside it, the mentions check.

import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, test, vi } from "vitest";
import ContextBar from "./ContextBar";

afterEach(() => {
  clearMocks();
  localStorage.clear();
});

test("the context bar checks for mentions in the picked project", async () => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_orgs") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "recent_mentions") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ContextBar
        org="acme"
        setOrg={() => {}}
        project="Web"
        setProject={() => {}}
        pbi={null}
        setPbi={() => {}}
        account={null}
        workMode={false}
        onToggleWork={() => {}}
        onOpenSettings={() => {}}
      />
    </QueryClientProvider>,
  );
  await waitFor(() =>
    expect(calls.find((c) => c.cmd === "recent_mentions")?.args).toEqual({ organization: "acme", project: "Web" }),
  );
  expect(calls.some((c) => c.cmd === "pr_overview")).toBe(true);
});

/// The gear turns forward as Settings opens and back as it closes, however
/// it closes (index.css, ico-cog-turn / ico-cog-turn-back). Each change is a
/// fresh icon, so the turn plays again; a first render plays nothing.
test("the Settings gear turns forward as Settings opens and back as it closes", () => {
  mockIPC((cmd) => {
    if (cmd === "list_orgs") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "recent_mentions") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const bar = (settingsOpen: boolean) => (
    <QueryClientProvider client={qc}>
      <ContextBar
        org="acme"
        setOrg={() => {}}
        project="Web"
        setProject={() => {}}
        pbi={null}
        setPbi={() => {}}
        account={null}
        workMode={false}
        onToggleWork={() => {}}
        onOpenSettings={() => {}}
        settingsOpen={settingsOpen}
      />
    </QueryClientProvider>
  );
  const { rerender } = render(bar(false));
  const icon = () => screen.getByRole("button", { name: /settings/i }).querySelector("svg")!;
  expect(icon()).not.toHaveClass("ico-cog-turn");
  expect(icon()).not.toHaveClass("ico-cog-turn-back");

  rerender(bar(true));
  const opened = icon();
  expect(opened).toHaveClass("ico-cog-turn");

  rerender(bar(false));
  expect(icon()).toHaveClass("ico-cog-turn-back");
  expect(icon()).not.toBe(opened);

  // A re-render that changes nothing does not replay it.
  const closed = icon();
  rerender(bar(false));
  expect(icon()).toBe(closed);
});

/// The PBI search takes ALL the free width between the Project picker and
/// the right-hand group: no max width, and the group never shrinks - the
/// search gives way first. jsdom does no layout, so this is the class contract.
test("the PBI search fills the free width and the right-hand group never shrinks", () => {
  mockIPC((cmd) => {
    if (cmd === "list_orgs") return [];
    if (cmd === "list_projects") return [];
    if (cmd === "pr_overview") return { mine: [], awaiting: [] };
    if (cmd === "recent_mentions") return [];
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <ContextBar
        org="acme"
        setOrg={() => {}}
        project="Web"
        setProject={() => {}}
        pbi={null}
        setPbi={() => {}}
        account={null}
        workMode={false}
        onToggleWork={() => {}}
        onOpenSettings={() => {}}
      />
    </QueryClientProvider>,
  );
  const box = screen.getByLabelText("Find PBI");
  const wrapper = box.closest("[data-tour='pbi']") as HTMLElement;
  // min-w-56 (224px), not wider: at a 1280px window the two pickers, this box
  // and the right-hand group must all fit on one line.
  expect(wrapper).toHaveClass("flex-1", "min-w-56");
  expect(wrapper).not.toHaveClass("min-w-80");
  expect(wrapper.className).not.toMatch(/(^|\s)max-w-/);
  expect(box).toHaveClass("h-10", "w-full");

  const right = screen.getByRole("button", { name: "Settings" }).parentElement!;
  expect(right).toHaveClass("shrink-0");
  expect(right).toContainElement(screen.getByRole("button", { name: /work manager/i }));

  // The two pickers keep their width but match the search's 40px height.
  for (const name of ["Organization", "Project"]) {
    expect(screen.getByRole("combobox", { name })).toHaveClass("h-10");
  }
});

/// The gear must sit centred whatever happens around it: the button is a
/// fixed square that centres its icon (so the icon never rides a text
/// baseline), the icon is a block, the turn pivots on its own centre, and
/// the focus ring is the app's, not the browser's default outline. jsdom does
/// no layout, so this pins the classes and the stylesheet rule.
describe("the Settings gear stays centred", () => {
  function bar(settingsOpen: boolean) {
    mockIPC((cmd) => {
      if (cmd === "list_orgs") return [];
      if (cmd === "list_projects") return [];
      if (cmd === "pr_overview") return { mine: [], awaiting: [] };
      if (cmd === "recent_mentions") return [];
    });
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    return (
      <QueryClientProvider client={qc}>
        <ContextBar
          org="acme"
          setOrg={() => {}}
          project="Web"
          setProject={() => {}}
          pbi={null}
          setPbi={() => {}}
          account={null}
          workMode={false}
          onToggleWork={() => {}}
          onOpenSettings={() => {}}
          settingsOpen={settingsOpen}
        />
      </QueryClientProvider>
    );
  }

  test.each([false, true])("with Settings open = %s", (open) => {
    render(bar(open));
    const button = screen.getByRole("button", { name: /settings/i });
    expect(button).toHaveClass("inline-flex", "size-8", "items-center", "justify-center", "rounded-md");
    expect(button).toHaveClass("focus-visible:outline-2", "focus-visible:outline-accent");
    expect(button.querySelector("svg")).toHaveClass("block");
  });

  test("the gear and its pressed state keep the centring across a toggle", () => {
    const { rerender } = render(bar(false));
    rerender(bar(true));
    const button = screen.getByRole("button", { name: /close settings/i });
    expect(button).toHaveClass("inline-flex", "size-8", "items-center", "justify-center");
    expect(button).toHaveClass("bg-accent-soft", "text-accent");
    expect(button.querySelector("svg")).toHaveClass("block", "ico-cog-turn");
  });

  test("the turn pivots about the icon's own centre", () => {
    const css = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "..", "index.css"), "utf-8");
    for (const cls of [".ico-cog-turn", ".ico-cog-turn-back"]) {
      const at = css.indexOf(`${cls} {`);
      const rule = at < 0 ? "" : css.slice(at, css.indexOf("}", at));
      expect(rule, cls).toMatch(/transform-origin:\s*center/);
      expect(rule, cls).toMatch(/transform-box:\s*fill-box/);
    }
  });
});

/// With exactly one organization the bar hides the Organization picker and
/// selects that organization itself (the same three calls the picker's
/// onChange makes). Zero or several keep the picker.
describe("a single organization is chosen for you", () => {
  function renderBar(opts: {
    orgs: Array<{ name: string; url: string }>;
    org: string;
    locked?: boolean;
  }) {
    mockIPC((cmd) => {
      if (cmd === "list_orgs") return opts.orgs;
      if (cmd === "list_projects") return [];
      if (cmd === "pr_overview") return { mine: [], awaiting: [] };
      if (cmd === "recent_mentions") return [];
    });
    const setOrg = vi.fn();
    const setProject = vi.fn();
    const setPbi = vi.fn();
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <ContextBar
          org={opts.org}
          setOrg={setOrg}
          project=""
          setProject={setProject}
          pbi={null}
          setPbi={setPbi}
          account={null}
          workMode={false}
          onToggleWork={() => {}}
          onOpenSettings={() => {}}
          locked={opts.locked}
        />
      </QueryClientProvider>,
    );
    return { setOrg, setProject, setPbi };
  }
  const orgBox = () => screen.queryByRole("combobox", { name: "Organization" });

  test("one organization hides the organization selector and selects it", async () => {
    const { setOrg, setProject, setPbi } = renderBar({ orgs: [{ name: "acme", url: "" }], org: "" });
    await waitFor(() => expect(setOrg).toHaveBeenCalledWith("acme"));
    expect(setProject).toHaveBeenCalledWith("");
    expect(setPbi).toHaveBeenCalledWith(null);
    expect(orgBox()).toBeNull();
  });

  test("one organization already selected is not selected again", async () => {
    const { setOrg } = renderBar({ orgs: [{ name: "acme", url: "" }], org: "acme" });
    await waitFor(() => expect(orgBox()).toBeNull());
    expect(setOrg).not.toHaveBeenCalled();
  });

  test("two organizations show the selector", async () => {
    const { setOrg } = renderBar({
      orgs: [
        { name: "acme", url: "" },
        { name: "Northwind", url: "" },
      ],
      org: "",
    });
    // Open the picker once; its options fill in when the list arrives.
    await waitFor(() => {
      const box = orgBox()!;
      if (box.getAttribute("aria-expanded") !== "true") fireEvent.click(box);
      expect(screen.getByRole("option", { name: "Northwind" })).toBeInTheDocument();
    });
    expect(screen.getByRole("option", { name: "acme" })).toBeInTheDocument();
    expect(setOrg).not.toHaveBeenCalled();
  });

  test("no organization keeps the selector", async () => {
    const { setOrg } = renderBar({ orgs: [], org: "" });
    await waitFor(() => expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument());
    expect(orgBox()).not.toBeNull();
    expect(setOrg).not.toHaveBeenCalled();
  });

  test("a locked bar never auto-selects", async () => {
    const { setOrg } = renderBar({ orgs: [{ name: "acme", url: "" }], org: "Northwind", locked: true });
    await waitFor(() => expect(orgBox()).toBeNull());
    expect(setOrg).not.toHaveBeenCalled();
  });

  test("the scope group always renders and never squeezes the search", async () => {
    renderBar({ orgs: [{ name: "acme", url: "" }], org: "acme" });
    await waitFor(() => expect(orgBox()).toBeNull());
    const group = document.querySelector("[data-tour='org']") as HTMLElement;
    expect(group).not.toBeNull();
    // min-w-56 (not min-w-0): the outer row must wrap the right-hand group
    // to a second row rather than squeeze the search.
    expect(group).toHaveClass("flex-1", "min-w-56");
    expect(group).toContainElement(document.querySelector("[data-tour='pbi']") as HTMLElement);
  });
});
