/**
 * ReportTree — areas → sections → objects navigator.
 * Pure presentational; selection flows through props so the editor owns state.
 */
import type { Ir, ObjectIr } from "./types";

interface Props {
  ir: Ir;
  selectedId: string | null;
  onSelectObject: (o: ObjectIr) => void;
  onSelectSection: (area: string, section: string) => void;
}

const kindIcon: Record<string, string> = {
  Text: "T",
  FieldHeading: "H",
  Field: "{ }",
  Box: "▭",
  Line: "／",
  Picture: "🖼",
  Subreport: "◫",
  Chart: "▲",
  CrossTab: "▦",
};

export function ReportTree({ ir, selectedId, onSelectObject, onSelectSection }: Props) {
  const byId = new Map(ir.objects.map((o) => [o.id, o]));
  return (
    <div className="h-full overflow-auto py-2 text-sm">
      {ir.areas.map((area) => (
        <div key={area.name} className="mb-1">
          <div className="px-3 py-1 text-xs font-semibold tracking-wide text-slate-500 uppercase">
            {area.name}
            {area.group_level != null && (
              <span className="ml-1 text-slate-400">G{area.group_level + 1}</span>
            )}
          </div>
          {area.sections.map((section) => (
            <div key={section.name}>
              <button
                className="block w-full px-3 py-0.5 text-left text-xs text-slate-400 hover:text-slate-200"
                onClick={() => onSelectSection(area.name, section.name)}
                title="Select section"
              >
                {section.suppress && <span className="mr-1 opacity-60">(suppressed)</span>}
                {section.name}
              </button>
              {section.object_ids.map((id) => {
                const o = byId.get(id);
                if (!o) return null;
                const selected = o.id === selectedId;
                return (
                  <button
                    key={id}
                    className={`flex w-full items-center gap-2 px-3 py-1 text-left ${
                      selected
                        ? "bg-sky-600/30 text-sky-100"
                        : "text-slate-200 hover:bg-slate-700/40"
                    }`}
                    onClick={() => onSelectObject(o)}
                  >
                    <span className="w-6 shrink-0 text-center text-[10px] text-slate-400">
                      {kindIcon[o.kind] ?? "•"}
                    </span>
                    <span className="truncate">
                      {o.name || <em className="text-slate-500">(unnamed)</em>}
                    </span>
                    <span className="ml-auto shrink-0 text-[10px] text-slate-500">{o.kind}</span>
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
