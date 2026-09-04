import { reportUpdateCheck } from "../lib/updateToast";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Kbd } from "@astryxdesign/core/Kbd";
import { Command } from "cmdk";
import { useEffect, useState } from "react";
import { commands } from "../bindings";
import { githubOffSnapshot } from "../lib/updatePrefs";
import AstryxIsland from "./AstryxIsland";
import { unwrap } from "../lib/ipc";
import { cached } from "../lib/localCache";
import { getTheme, setTheme } from "../lib/theme";
import { tourRunningSnapshot } from "../tour/tourState";
import type { Section } from "./Sidebar";

export default function CommandPalette({
  onNavigate,
  org,
  onSwitchProject,
  onToggleWork,
}: {
  onNavigate: (s: Section) => void;
  org: string;
  onSwitchProject: (p: string) => void;
  onToggleWork: () => void;
}) {
  const qc = useQueryClient();
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (tourRunningSnapshot()) return;
      if (e.key.toLowerCase() === "k" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Same key + cache as ContextBar, so the palette never refetches what
  // the bar already has.
  const projects = useQuery({
    queryKey: ["projects", org],
    queryFn: () =>
      cached(`projects:${org}`, 24 * 60 * 60_000, () => unwrap(commands.listProjects(org))),
    enabled: open && Boolean(org),
    staleTime: 60 * 60_000,
  });

  const run = (fn: () => void) => {
    fn();
    setOpen(false);
  };

  return (
    <Command.Dialog
      open={open}
      onOpenChange={setOpen}
      label="Command palette"
      className="fixed left-1/2 top-24 z-50 w-[520px] -translate-x-1/2 overflow-hidden rounded-lg border border-border bg-surface shadow-2xl"
      overlayClassName="fixed inset-0 z-40 bg-black/40"
    >
      {/* Island so the Kbd badges pick up the app-token Astryx theme. */}
      <AstryxIsland>
      <Command.Input
        placeholder="Type a command or search"
        className="w-full border-b border-border bg-transparent px-4 py-3 text-sm text-text outline-none placeholder:text-faint"
      />
      <Command.List className="max-h-72 overflow-y-auto p-2 text-sm">
        <Command.Empty className="px-3 py-6 text-center text-muted">
          No results.
        </Command.Empty>

        <Command.Group heading="Go to" className="px-1 text-[10px] uppercase tracking-wide text-faint">
          <Item keys="mod+1" onSelect={() => run(() => onNavigate("manual"))}>Manual Entry</Item>
          <Item keys="mod+2" onSelect={() => run(() => onNavigate("import"))}>Import File</Item>
          <Item keys="mod+3" onSelect={() => run(() => onNavigate("edit"))}>Update Test Cases</Item>
          <Item keys="mod+4" onSelect={() => run(() => onNavigate("run"))}>Run Tests</Item>
          <Item keys="mod+5" onSelect={() => run(() => onNavigate("suites"))}>Test Suites</Item>
          <Item onSelect={() => run(() => onNavigate("settings"))}>Settings</Item>
        </Command.Group>

        <Command.Group heading="Actions" className="px-1 text-[10px] uppercase tracking-wide text-faint">
          <Item keys="mod+shift+m" onSelect={() => run(onToggleWork)}>Toggle Work Manager</Item>
          <Item
            onSelect={() =>
              run(() => setTheme(getTheme() === "light" ? "dark" : "light"))
            }
          >
            Toggle theme
          </Item>
          <Item
            onSelect={() =>
              run(async () => {
                const v = await commands.checkUpdate(githubOffSnapshot());
                // Seed the ["update"] query so App's update banner appears.
                qc.setQueryData(["update"], v);
                reportUpdateCheck(v);
              })
            }
          >
            Check for updates
          </Item>
        </Command.Group>

        {org && (projects.data?.length ?? 0) > 0 && (
          <Command.Group heading="Switch project" className="px-1 text-[10px] uppercase tracking-wide text-faint">
            {projects.data!.map((p) => (
              <Item key={p.id} onSelect={() => run(() => onSwitchProject(p.name))}>
                {p.name}
              </Item>
            ))}
          </Command.Group>
        )}
      </Command.List>
      </AstryxIsland>
    </Command.Dialog>
  );
}

function Item({
  children,
  onSelect,
  keys,
}: {
  children: React.ReactNode;
  onSelect: () => void;
  /** Astryx Kbd shortcut string, e.g. "mod+1" - shown right-aligned. */
  keys?: string;
}) {
  return (
    <Command.Item
      onSelect={onSelect}
      className="flex cursor-pointer items-center justify-between rounded-md px-3 py-2 text-sm text-text aria-selected:bg-accent-soft aria-selected:text-accent"
    >
      {children}
      {keys && <Kbd keys={keys} />}
    </Command.Item>
  );
}
