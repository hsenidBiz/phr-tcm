import { ClipboardList, KanbanSquare, Settings as SettingsIcon } from "lucide-react";
import { cn } from "../lib/cn";

export type Section = "tests" | "work" | "settings";

const ITEMS: { id: Section; label: string; icon: typeof ClipboardList }[] = [
  { id: "tests", label: "Test Cases", icon: ClipboardList },
  { id: "work", label: "Work", icon: KanbanSquare },
  { id: "settings", label: "Settings", icon: SettingsIcon },
];

export default function Sidebar({
  section,
  onSelect,
}: {
  section: Section;
  onSelect: (s: Section) => void;
}) {
  return (
    <nav className="flex h-full w-48 flex-col gap-1 border-r border-border bg-surface p-2">
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
