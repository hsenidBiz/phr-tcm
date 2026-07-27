import {
  Bot,
  ChevronsLeft,
  ChevronsRight,
  Eye,
  FilePlus2,
  FileUp,
  FolderTree,
  GitPullRequest,
  KanbanSquare,
  PenLine,
  Pencil,
  PlayCircle,
} from "lucide-react";
import { useState } from "react";
import { Tooltip } from "./ui/tooltip";
import { cn } from "../lib/cn";

/** The v1 tabs, one screen each. Settings and the Work Manager switch live
 * in the context bar. Collapsible to an icon rail. In Work Manager mode the
 * same rail shows WORK_ITEMS instead (PRs first, then the board). */
export type Section = "manual" | "import" | "edit" | "view" | "run" | "suites" | "ai" | "settings";
export type WorkSection = "prs" | "board" | "create";

type Item<T extends string> = {
  id: T;
  label: string;
  icon: typeof PenLine;
  /** The icon's own colour, so the rail is scannable by hue when it is
   * collapsed to icons alone. The selected row's tinted background stays
   * the theme accent - only the glyph is coloured. */
  tone: string;
};

const CASE_ITEMS: Item<Section>[] = [
  { id: "manual", label: "Manual Entry", icon: PenLine, tone: "nav-ico nav-ico-manual" },
  { id: "import", label: "Import File", icon: FileUp, tone: "nav-ico nav-ico-import" },
  { id: "edit", label: "Update Test Cases", icon: Pencil, tone: "nav-ico nav-ico-edit" },
  { id: "view", label: "View Test Cases", icon: Eye, tone: "nav-ico nav-ico-view" },
  { id: "run", label: "Run Tests", icon: PlayCircle, tone: "nav-ico nav-ico-run" },
  { id: "suites", label: "Test Suites", icon: FolderTree, tone: "nav-ico nav-ico-suites" },
  { id: "ai", label: "AI Bridge", icon: Bot, tone: "nav-ico nav-ico-ai" },
];

export const WORK_ITEMS: Item<WorkSection>[] = [
  { id: "prs", label: "Pull Requests", icon: GitPullRequest, tone: "nav-ico nav-ico-prs" },
  { id: "board", label: "Board", icon: KanbanSquare, tone: "nav-ico nav-ico-board" },
  { id: "create", label: "New Work Item", icon: FilePlus2, tone: "nav-ico nav-ico-create" },
];

const COLLAPSE_KEY = "tcm-v2-sidebar";

export default function Sidebar<T extends string = Section>({
  section,
  onSelect,
  items,
}: {
  section: T;
  onSelect: (s: T) => void;
  /** Defaults to the test-case tabs; Work Manager passes WORK_ITEMS. */
  items?: Item<T>[];
}) {
  const list = items ?? (CASE_ITEMS as unknown as Item<T>[]);
  const [collapsed, setCollapsed] = useState(
    () => localStorage.getItem(COLLAPSE_KEY) === "collapsed",
  );
  const toggle = () => {
    setCollapsed((c) => {
      const next = !c;
      try {
        localStorage.setItem(COLLAPSE_KEY, next ? "collapsed" : "open");
      } catch {
        // storage unavailable -> session-only
      }
      return next;
    });
  };

  /** Labels stay mounted and never wrap: collapsing fades them out FIRST,
   * then the width animates (reversed when expanding), so text is clipped
   * behind an invisible curtain instead of squishing as the rail narrows. */
  const labelCls = cn(
    "overflow-hidden whitespace-nowrap transition-[opacity,max-width,margin-left] duration-200",
    collapsed ? "ml-0 max-w-0 opacity-0" : "ml-2.5 max-w-40 opacity-100",
  );
  const labelDelay = { transitionDelay: collapsed ? "0ms" : "120ms" };

  return (
    <nav
      className={cn(
        "flex h-full shrink-0 flex-col gap-1 border-r border-border bg-surface p-2 transition-[width] duration-200",
        collapsed ? "w-14" : "w-52",
      )}
      style={{ transitionDelay: collapsed ? "120ms" : "0ms" }}
    >
      {list.map(({ id, label, icon: Icon, tone }) => (
        // Only when collapsed: with the rail open the label is right there.
        <Tooltip key={id} label={label} side="right" disabled={!collapsed}>
        <button
          data-tour={`nav-${id}`}
          onClick={() => onSelect(id)}
          aria-current={section === id ? "page" : undefined}
          aria-label={label}
          className={cn(
            // px-3 keeps the icon at the exact same x whether the rail is
            // wide or collapsed (8px nav pad + 12px = centered in w-14), so
            // icons stay perfectly still while the width animates.
            "group flex items-center overflow-hidden rounded-md px-3 py-2 text-left text-sm transition-colors",
            section === id
              ? "bg-accent-soft font-medium text-accent"
              : "text-muted hover:bg-surface-2 hover:text-text",
          )}
        >
          <Icon
            size={16}
            className={cn(
              "shrink-0 transition-colors",
              // Selected rows keep the accent so the highlight reads as
              // one block; unselected rows carry the icon's own colour,
              // dimmed until hover so the rail stays calm.
              section === id ? "nav-ico-on text-accent" : tone,
            )}
          />
          <span className={labelCls} style={labelDelay}>
            {label}
          </span>
        </button>
        </Tooltip>
      ))}
      <div className="mt-auto space-y-1">
        <div
          className={cn(
            "overflow-hidden whitespace-nowrap px-3 text-[10px] text-faint transition-[opacity,max-height,padding] duration-200",
            collapsed ? "max-h-0 py-0 opacity-0" : "max-h-6 py-1 opacity-100",
          )}
          style={labelDelay}
        >
          Ctrl+K for commands
        </div>
        <button
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          className="flex w-full items-center overflow-hidden rounded-md px-3 py-2 text-sm text-faint hover:bg-surface-2 hover:text-text"
          onClick={toggle}
        >
          {collapsed ? <ChevronsRight size={15} className="shrink-0" /> : <ChevronsLeft size={15} className="shrink-0" />}
          <span className={labelCls} style={labelDelay}>
            Collapse
          </span>
        </button>
      </div>
    </nav>
  );
}
