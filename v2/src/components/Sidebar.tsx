import {
  FileUp,
  FolderTree,
  PenLine,
  Pencil,
  PlayCircle,
} from "lucide-react";
import { cn } from "../lib/cn";

/** The v1 tabs, one screen each. Settings and the Work Manager switch live
 * in the context bar - the sidebar stays reserved for test-case workflows
 * so new tabs can be added over time. */
export type Section = "manual" | "import" | "edit" | "run" | "suites" | "settings";

const ITEMS: { id: Section; label: string; icon: typeof PenLine }[] = [
  { id: "manual", label: "Manual Entry", icon: PenLine },
  { id: "import", label: "Import File", icon: FileUp },
  { id: "edit", label: "Edit Test Cases", icon: Pencil },
  { id: "run", label: "Run Tests", icon: PlayCircle },
  { id: "suites", label: "Test Suites", icon: FolderTree },
];

export default function Sidebar({
  section,
  onSelect,
}: {
  section: Section;
  onSelect: (s: Section) => void;
}) {
  return (
    <nav className="flex h-full w-52 flex-col gap-1 border-r border-border bg-surface p-2">
      <div className="px-3 py-3 text-sm font-semibold text-text">
        Test Case Manager
        <span className="ml-1 rounded bg-accent-soft px-1 text-[10px] font-semibold text-accent">
          V2
        </span>
      </div>
      {ITEMS.map(({ id, label, icon: Icon }) => (
        <button
          key={id}
          onClick={() => onSelect(id)}
          aria-current={section === id ? "page" : undefined}
          className={cn(
            "flex items-center gap-2.5 rounded-md px-3 py-2 text-left text-sm transition-colors",
            section === id
              ? "bg-accent-soft font-medium text-accent"
              : "text-muted hover:bg-surface-2 hover:text-text",
          )}
        >
          <Icon size={16} />
          {label}
        </button>
      ))}
      <div className="mt-auto px-3 py-2 text-[10px] text-faint">
        Ctrl+K for commands
      </div>
    </nav>
  );
}
