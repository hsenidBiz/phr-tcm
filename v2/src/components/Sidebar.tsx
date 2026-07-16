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

  return (
    <nav
      className={cn(
        "flex h-full shrink-0 flex-col gap-1 border-r border-border bg-surface p-2 transition-[width] duration-200",
        collapsed ? "w-14" : "w-52",
      )}
    >
      {ITEMS.map(({ id, label, icon: Icon }) => (
        <button
          key={id}
          data-tour={`nav-${id}`}
          onClick={() => onSelect(id)}
          aria-current={section === id ? "page" : undefined}
          title={label}
          className={cn(
            "flex items-center gap-2.5 rounded-md px-3 py-2 text-left text-sm transition-colors",
            collapsed && "justify-center px-0",
            section === id
              ? "bg-accent-soft font-medium text-accent"
              : "text-muted hover:bg-surface-2 hover:text-text",
          )}
        >
          <Icon size={16} className="shrink-0" />
          {!collapsed && label}
        </button>
      ))}
      <div className="mt-auto space-y-1">
        {!collapsed && (
          <div className="px-3 py-1 text-[10px] text-faint">Ctrl+K for commands</div>
        )}
        <button
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          title={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          className={cn(
            "flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-sm text-faint hover:bg-surface-2 hover:text-text",
            collapsed && "justify-center px-0",
          )}
          onClick={toggle}
        >
          {collapsed ? <ChevronsRight size={15} /> : <ChevronsLeft size={15} />}
          {!collapsed && "Collapse"}
        </button>
      </div>
    </nav>
  );
}
