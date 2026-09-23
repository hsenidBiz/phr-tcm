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

/** True if every character of `needle` occurs in `haystack` in the same
 * order, gaps allowed - a subsequence match, case-insensitive. This is the
 * matching cmdk did before the move to XiodUI; Base UI's own filter is a
 * plain substring `contains`, which "chk upd" or "tgl theme" would fail. */
function isSubsequence(needle: string, haystack: string): boolean {
  if (needle === "") return true;
  let i = 0;
  for (const ch of haystack) {
    if (ch === needle[i]) i += 1;
    if (i === needle.length) return true;
  }
  return false;
}

/** The palette's search: a row matches if the query's letters appear in
 * its label in order, spaces in the query ignored ("chk upd" -> "Check for
 * updates"). Base UI's `filter` is boolean-only - it has no ranking hook -
 * so matching rows stay in the order they were declared rather than being
 * sorted by match quality the way cmdk used to.
 *
 * XiodUI's `Command` collapses `Autocomplete`'s generic item type to
 * `unknown` (`React.ComponentProps<typeof Autocomplete>` erases it), so
 * `item` is cast back to `Entry` - safe, since every item this palette
 * renders comes from `groups` below. */
function paletteFilter(item: unknown, query: string): boolean {
  const entry = item as Entry;
  return isSubsequence(query.toLowerCase().replace(/\s+/g, ""), entry.label.toLowerCase());
}

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
        <Command items={groups} filter={paletteFilter}>
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
