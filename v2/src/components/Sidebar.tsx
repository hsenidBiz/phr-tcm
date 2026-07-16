import {
  ChevronsLeft,
  ChevronsRight,
  Eye,
  FileUp,
  FolderTree,
  PenLine,
  Pencil,
  PlayCircle,
} from "lucide-react";
import { useState } from "react";
import { cn } from "../lib/cn";

/** The v1 tabs, one screen each. Settings and the Work Manager switch live
 * in the context bar - the sidebar stays reserved for test-case workflows
 * so new tabs can be added over time. Collapsible to an icon rail. */
export type Section = "manual" | "import" | "edit" | "view" | "run" | "suites" | "settings";

const ITEMS: { id: Section; label: string; icon: typeof PenLine }[] = [
  { id: "manual", label: "Manual Entry", icon: PenLine },
  { id: "import", label: "Import File", icon: FileUp },
  { id: "edit", label: "Edit Test Cases", icon: Pencil },
  { id: "view", label: "View Test Cases", icon: Eye },
  { id: "run", label: "Run Tests", icon: PlayCircle },
  { id: "suites", label: "Test Suites", icon: FolderTree },
];

const COLLAPSE_KEY = "tcm-v2-sidebar";

export default function Sidebar({
  section,
  onSelect,
}: {
  section: Section;
  onSelect: (s: Section) => void;
}) {
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
      {ITEMS.map(({ id, label, icon: Icon }) => (
        <button
          key={id}
          data-tour={`nav-${id}`}
          onClick={() => onSelect(id)}
          aria-current={section === id ? "page" : undefined}
          title={label}
          className={cn(
            // px-3 keeps the icon at the exact same x whether the rail is
            // wide or collapsed (8px nav pad + 12px = centered in w-14), so
            // icons stay perfectly still while the width animates.
            "flex items-center overflow-hidden rounded-md px-3 py-2 text-left text-sm transition-colors",
            section === id
              ? "bg-accent-soft font-medium text-accent"
              : "text-muted hover:bg-surface-2 hover:text-text",
          )}
        >
          <Icon size={16} className="shrink-0" />
          <span className={labelCls} style={labelDelay}>
            {label}
          </span>
        </button>
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
