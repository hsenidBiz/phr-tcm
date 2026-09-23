import { reportUpdateCheck } from "../lib/updateToast";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Command,
  CommandCollection,
  CommandDialog,
  CommandDialogPopup,
  CommandEmpty,
  CommandGroup,
  CommandGroupLabel,
  CommandInput,
  CommandItem,
  CommandList,
  CommandPanel,
} from "xiod-ui/command";
import { useEffect, useState } from "react";
import { commands } from "../bindings";
import { Kbd } from "./ui/kbd";
import { unwrap } from "../lib/ipc";
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { getTheme, setTheme } from "../lib/theme";
import { tourRunningSnapshot } from "../tour/tourState";
import { VISIBLE_CASE_ITEMS, sectionShortcut, type Section } from "./Sidebar";

/** One row. `value` is unique across the whole palette; `label` is what
 * shows and what typing filters on; `keys` is its shortcut hint. */
type Entry = { value: string; label: string; keys?: string; run: () => void };
type Group = { value: string; items: Entry[] };

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
    ...persistentQuery({
      key: cacheKeys.projects(org),
      fetcher: () => unwrap(commands.listProjects(org)),
      ...CACHE.reference,
    }),
    enabled: open && Boolean(org),
  });

  const run = (fn: () => void) => {
    fn();
    setOpen(false);
  };

  const groups: Group[] = [
    {
      value: "Go to",
      items: [
        // One row per sidebar tab, hint digit = its Ctrl+N slot, both read
        // off the same list App's shortcut handler uses.
        ...VISIBLE_CASE_ITEMS.map((i) => ({
          value: `go:${i.id}`,
          label: i.label,
          keys: sectionShortcut(i.id),
          run: () => onNavigate(i.id),
        })),
        { value: "go:settings", label: "Settings", run: () => onNavigate("settings") },
      ],
    },
    {
      value: "Actions",
      items: [
        { value: "action:work", label: "Toggle Work Manager", keys: "mod+shift+m", run: onToggleWork },
        {
          value: "action:theme",
          label: "Toggle theme",
          run: () => setTheme(getTheme() === "light" ? "dark" : "light"),
        },
        {
          value: "action:update",
          label: "Check for updates",
          run: async () => {
            const v = await commands.checkUpdate();
            // Seed the ["update"] query so App's update banner appears.
            qc.setQueryData(["update"], v);
            reportUpdateCheck(v);
          },
        },
      ],
    },
  ];
  if (org && (projects.data?.length ?? 0) > 0) {
    groups.push({
      value: "Switch project",
      items: projects.data!.map((p) => ({ value: `project:${p.id}`, label: p.name, run: () => onSwitchProject(p.name) })),
    });
  }

  return (
    <CommandDialog open={open} onOpenChange={setOpen}>
      <CommandDialogPopup aria-label="Command palette">
        <Command items={groups}>
          <CommandInput placeholder="Type a command or search" />
          <CommandPanel>
            <CommandEmpty>No results.</CommandEmpty>
            <CommandList>
              {(group: Group) => (
                <CommandGroup key={group.value} items={group.items}>
                  <CommandGroupLabel className="text-[10px] uppercase tracking-wide text-faint">
                    {group.value}
                  </CommandGroupLabel>
                  <CommandCollection>
                    {(item: Entry) => (
                      <CommandItem
                        key={item.value}
                        value={item}
                        onClick={() => run(item.run)}
                        className="cursor-pointer justify-between gap-3 text-text"
                      >
                        {item.label}
                        {item.keys && <Kbd keys={item.keys} />}
                      </CommandItem>
                    )}
                  </CommandCollection>
                </CommandGroup>
              )}
            </CommandList>
          </CommandPanel>
        </Command>
      </CommandDialogPopup>
    </CommandDialog>
  );
}
