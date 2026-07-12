import { useQuery } from "@tanstack/react-query";
import { Command } from "cmdk";
import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands } from "../bindings";
import { unwrap } from "../lib/ipc";
import { getTheme, setTheme } from "../lib/theme";
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
  const [open, setOpen] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "k" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const projects = useQuery({
    queryKey: ["projects", org],
    queryFn: () => unwrap(commands.listProjects(org)),
    enabled: open && Boolean(org),
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
      <Command.Input
        placeholder="Type a command or search..."
        className="w-full border-b border-border bg-transparent px-4 py-3 text-sm text-text outline-none placeholder:text-faint"
      />
      <Command.List className="max-h-72 overflow-y-auto p-2 text-sm">
        <Command.Empty className="px-3 py-6 text-center text-muted">
          No results.
        </Command.Empty>

        <Command.Group heading="Go to" className="px-1 text-[10px] uppercase tracking-wide text-faint">
          <Item onSelect={() => run(() => onNavigate("manual"))}>Manual Entry</Item>
          <Item onSelect={() => run(() => onNavigate("import"))}>Import File</Item>
          <Item onSelect={() => run(() => onNavigate("edit"))}>Edit Test Cases</Item>
          <Item onSelect={() => run(() => onNavigate("run"))}>Run Tests</Item>
          <Item onSelect={() => run(() => onNavigate("suites"))}>Test Suites</Item>
          <Item onSelect={() => run(() => onNavigate("settings"))}>Settings</Item>
        </Command.Group>

        <Command.Group heading="Actions" className="px-1 text-[10px] uppercase tracking-wide text-faint">
          <Item onSelect={() => run(onToggleWork)}>Toggle Work Manager</Item>
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
                const v = await commands.checkUpdate();
                if (v) toast.info(`Version ${v} is available.`);
                else toast.success("You are on the latest version.");
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
    </Command.Dialog>
  );
}

function Item({
  children,
  onSelect,
}: {
  children: React.ReactNode;
  onSelect: () => void;
}) {
  return (
    <Command.Item
      onSelect={onSelect}
      className="cursor-pointer rounded-md px-3 py-2 text-sm text-text aria-selected:bg-accent-soft aria-selected:text-accent"
    >
      {children}
    </Command.Item>
  );
}
