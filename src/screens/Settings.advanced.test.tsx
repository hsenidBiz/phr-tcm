// Settings' Enable Advanced Features switch: shows Auto Run and API
// Templates (and their AI tools) the way a development build always does,
// saved through Rust, and nothing else.
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";

const NAME = "Enable Advanced Features";
const DESCRIPTION = "Shows Auto Run and API Templates, and the AI tools that go with them.";
const SAVE_FAILED = "Could not save this setting. The app log in Settings has the details.";

afterEach(() => {
  clearMocks();
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  vi.resetModules();
  localStorage.clear();
});

/** Rust's side of both flags, in memory. Returns every advanced value saved. */
function ipc({ advanced = false, failSaves = false } = {}) {
  const saves: boolean[] = [];
  let on = advanced;
  mockIPC((cmd, args) => {
    if (cmd === "get_extras_unlocked") return false;
    if (cmd === "get_advanced_features") return on;
    if (cmd === "set_advanced_features") {
      if (failSaves) throw SAVE_FAILED;
      on = (args as { on: boolean }).on;
      saves.push(on);
      return null;
    }
    return undefined;
  });
  return saves;
}

/** Settings beside the sidebar, from a fresh module graph - a release build
 * (DEV off) when asked, where the switch is the only thing showing Auto Run. */
async function renderScreen({ release }: { release: boolean }) {
  if (release) vi.stubEnv("DEV", false);
  vi.resetModules();
  const { QueryClient, QueryClientProvider } = await import("@tanstack/react-query");
  const Settings = (await import("./Settings")).default;
  const Sidebar = (await import("../components/Sidebar")).default;
  const { toast } = await import("../lib/toast");
  const failure = vi.spyOn(toast, "error").mockImplementation((() => undefined) as never);
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(
    <QueryClientProvider client={qc}>
      <div data-testid="rail">
        <Sidebar section="manual" onSelect={() => {}} />
      </div>
      <Settings org="acme" project="Web" />
    </QueryClientProvider>,
  );
  const rail = within(screen.getByTestId("rail"));
  return { view, rail, failure };
}

const theSwitch = () => screen.getByRole("switch", { name: NAME });

test("the row sits in General with its name and description, off by default", async () => {
  ipc();
  const { view } = await renderScreen({ release: false });
  const general = view.container.querySelector('[data-settings-card="general"]') as HTMLElement;
  expect(general).not.toBeNull();
  expect(within(general).getByRole("switch", { name: NAME })).toHaveAttribute("aria-checked", "false");
  expect(within(general).getByText(NAME)).toBeInTheDocument();
  expect(within(general).getByText(DESCRIPTION)).toBeInTheDocument();
});

test("in a release build, turning it on saves it and shows Auto Run and API Templates; off hides them", async () => {
  const saves = ipc();
  const { rail } = await renderScreen({ release: true });
  await waitFor(() => expect(theSwitch()).toHaveAttribute("aria-checked", "false"));
  expect(rail.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  expect(rail.queryByRole("button", { name: /API Templates/ })).not.toBeInTheDocument();

  fireEvent.click(theSwitch());
  await waitFor(() => expect(theSwitch()).toHaveAttribute("aria-checked", "true"));
  expect(saves).toEqual([true]);
  expect(rail.getByRole("button", { name: /Auto Run/ })).toBeInTheDocument();
  expect(rail.getByRole("button", { name: /API Templates/ })).toBeInTheDocument();

  fireEvent.click(theSwitch());
  await waitFor(() => expect(theSwitch()).toHaveAttribute("aria-checked", "false"));
  expect(saves).toEqual([true, false]);
  expect(rail.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
  expect(rail.queryByRole("button", { name: /API Templates/ })).not.toBeInTheDocument();
});

test("a saved on is read back at start", async () => {
  ipc({ advanced: true });
  const { rail } = await renderScreen({ release: true });
  await waitFor(() => expect(theSwitch()).toHaveAttribute("aria-checked", "true"));
  expect(rail.getByRole("button", { name: /Auto Run/ })).toBeInTheDocument();
});

test("it does not bring the Extras card with it", async () => {
  ipc();
  const { view } = await renderScreen({ release: true });
  fireEvent.click(theSwitch());
  await waitFor(() => expect(theSwitch()).toHaveAttribute("aria-checked", "true"));
  expect(screen.queryByRole("heading", { name: "Extras" })).not.toBeInTheDocument();
  expect(view.container.querySelector('[data-settings-card="extras"]')).toBeNull();
});

// The help site documents the row, so capture mode shows it - on, as the
// sample data answers - with Auto Run and API Templates beside it. The
// Extras card stays out (Settings.test.tsx pins that).
test("capture mode shows the row, on, with Auto Run and API Templates in the rail", async () => {
  localStorage.setItem("tcm-v2-dev-capture", "on");
  ipc({ advanced: true });
  const { rail } = await renderScreen({ release: false });
  await waitFor(() => expect(theSwitch()).toHaveAttribute("aria-checked", "true"));
  expect(screen.getByText(DESCRIPTION)).toBeInTheDocument();
  expect(rail.getByRole("button", { name: /Auto Run/ })).toBeInTheDocument();
  expect(rail.getByRole("button", { name: /API Templates/ })).toBeInTheDocument();
  expect(screen.queryByRole("heading", { name: "Extras" })).not.toBeInTheDocument();
});

test("a save that fails leaves the switch off and says so", async () => {
  const saves = ipc({ failSaves: true });
  const { rail, failure } = await renderScreen({ release: true });
  fireEvent.click(theSwitch());
  await waitFor(() => expect(failure).toHaveBeenCalledWith(SAVE_FAILED));
  expect(saves).toEqual([]);
  expect(theSwitch()).toHaveAttribute("aria-checked", "false");
  expect(rail.queryByRole("button", { name: /Auto Run/ })).not.toBeInTheDocument();
});
