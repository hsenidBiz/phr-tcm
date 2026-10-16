/**
 * LayoutPreview — an SVG canvas of the report's design surface, stacked by area/section
 * with objects placed from their twip bounds (scaled px-per-inch, default 96 = CSS inch).
 * Click an object to select it; the selected object gets a sky outline.
 *
 * This is a *layout* preview (where things are), not a render (what data looks like) —
 * that distinction is the point of the v1 editor.
 */
import { useMemo, useState } from "react";
import type { Ir, ObjectIr } from "./types";
import { twipsToPx } from "./types";

interface Props {
  ir: Ir;
  selectedId: string | null;
  onSelectObject: (o: ObjectIr) => void;
  pxPerInch?: number;
}

export function LayoutPreview({ ir, selectedId, onSelectObject, pxPerInch = 96 }: Props) {
  const [zoom, setZoom] = useState(1);
  const scale = pxPerInch * zoom;

  const rows = useMemo(() => {
    const byId = new Map(ir.objects.map((o) => [o.id, o]));
    return ir.areas.flatMap((area) =>
      area.sections.map((section) => ({
        key: `${area.name}/${section.name}`,
        label: section.name,
        height: twipsToPx(section.height, scale),
        objects: section.object_ids
          .map((id) => byId.get(id))
          .filter((o): o is ObjectIr => !!o),
      })),
    );
  }, [ir, scale]);

  const width = twipsToPx(ir.print_options.content_width || 612 * 20, scale);

  return (
    <div className="relative h-full overflow-auto bg-slate-900 p-6">
      <div className="absolute top-2 right-4 z-10 flex items-center gap-2 text-xs text-slate-300">
        <button
          className="rounded border border-slate-600 px-2 py-0.5 hover:bg-slate-700/40"
          onClick={() => setZoom((z) => Math.max(0.25, z - 0.25))}
        >
          −
        </button>
        <span>{Math.round(zoom * 100)}%</span>
        <button
          className="rounded border border-slate-600 px-2 py-0.5 hover:bg-slate-700/40"
          onClick={() => setZoom((z) => Math.min(3, z + 0.25))}
        >
          +
        </button>
      </div>

      <div
        className="mx-auto bg-white shadow-2xl"
        style={{ width }}
        role="presentation"
      >
        {rows.map((row) => (
          <div
            key={row.key}
            className="relative border-b border-dashed border-slate-300"
            style={{ height: Math.max(row.height, 18) }}
          >
            <span className="absolute top-0.5 left-1 z-[1] bg-white/80 text-[9px] text-slate-400">
              {row.label}
            </span>
            {row.objects.map((o) => {
              const selected = o.id === selectedId;
              const x = twipsToPx(o.bounds.left, scale);
              const y = twipsToPx(o.bounds.top, scale);
              const w = Math.max(twipsToPx(o.bounds.width, scale), 4);
              const h = Math.max(twipsToPx(o.bounds.height, scale), 4);
              const label = o.text?.display || o.field?.data_source || o.name || o.kind;
              return (
                <button
                  key={o.id}
                  title={`${o.name} (${o.kind})`}
                  onClick={() => onSelectObject(o)}
                  className={`absolute overflow-hidden text-left ${
                    selected
                      ? "outline outline-2 outline-sky-500"
                      : "hover:outline hover:outline-1 hover:outline-sky-300"
                  }`}
                  style={{ left: x, top: y, width: w, height: h }}
                >
                  <span
                    className="block truncate leading-none"
                    style={{
                      fontSize: Math.max(8, twipsToPx((o.text?.font.size_pt ?? 8) * 20, scale)),
                      fontFamily: o.text?.font.name,
                      fontWeight: o.text?.font.bold ? 700 : 400,
                      fontStyle: o.text?.font.italic ? "italic" : "normal",
                      color: o.text
                        ? `rgb(${o.text.color.r},${o.text.color.g},${o.text.color.b})`
                        : undefined,
                    }}
                  >
                    {label}
                  </span>
                </button>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
