/**
 * PropertyInspector — edit forms for the v1 scope: text content, position/size,
 * fonts & colors, section height, page setup. Collects changes into an op batch
 * and calls onApply; the editor then applies, refreshes the IR, and re-renders.
 *
 * Inputs use points/inches for humans; ops are sent in twips via the converters.
 */
import { useState } from "react";
import type { EditOp, Ir, ObjectIr } from "./types";
import { inchesToTwips, pointsToTwips, twipsToInches, twipsToPoints } from "./types";

export interface Selection {
  object: ObjectIr | null;
  section: { area: string; section: string } | null;
}

interface Props {
  ir: Ir;
  selection: Selection;
  onApply: (ops: EditOp[]) => Promise<void>;
  busy: boolean;
  error: string | null;
}

const label = "block text-xs font-medium text-slate-400 mb-1";
const input =
  "w-full rounded border border-slate-600 bg-slate-800 px-2 py-1 text-sm text-slate-100 " +
  "focus:border-sky-500 focus:outline-none disabled:opacity-40";
const btn =
  "rounded bg-sky-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-sky-500 " +
  "disabled:cursor-not-allowed disabled:opacity-50";

export function PropertyInspector({ ir, selection, onApply, busy, error }: Props) {
  const [pending, setPending] = useState<EditOp[]>([]);
  const add = (op: EditOp) =>
    setPending((p) => [...p.filter((q) => !sameSlot(q, op)), op]);
  const section = selection.section
    ? ir.areas
        .find((a) => a.name === selection.section!.area)
        ?.sections.find((s) => s.name === selection.section!.section) ?? null
    : null;

  return (
    <div className="flex h-full flex-col">
      <div className="flex-1 space-y-5 overflow-auto p-3 text-sm">
        {error && (
          <div className="rounded border border-red-500/40 bg-red-500/10 p-2 text-xs text-red-300">
            {error}
          </div>
        )}

        {selection.object ? (
          <ObjectForm object={selection.object} add={add} />
        ) : selection.section && section ? (
          <SectionForm
            area={selection.section.area}
            name={section.name}
            heightTw={section.height}
            add={add}
          />
        ) : (
          <PageForm ir={ir} add={add} />
        )}
      </div>

      <div className="flex items-center gap-2 border-t border-slate-700 p-3">
        <span className="text-xs text-slate-400">
          {pending.length} pending op{pending.length === 1 ? "" : "s"}
        </span>
        <button
          className={`${btn} ml-auto`}
          disabled={busy || pending.length === 0}
          onClick={async () => {
            const ops = pending;
            setPending([]);
            await onApply(ops);
          }}
        >
          Apply
        </button>
        <button
          className="rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700/40"
          disabled={pending.length === 0}
          onClick={() => setPending([])}
        >
          Discard
        </button>
      </div>
    </div>
  );
}

function ObjectForm({ object: o, add }: { object: ObjectIr; add: (op: EditOp) => void }) {
  const t = o.text;
  const [text, setText] = useState(t?.text ?? "");
  const [fontName, setFontName] = useState(t?.font.name ?? "");
  const [sizePt, setSizePt] = useState(t?.font.size_pt ? String(t.font.size_pt) : "");
  const [bold, setBold] = useState(!!t?.font.bold);
  const [italic, setItalic] = useState(!!t?.font.italic);
  const [leftIn, setLeftIn] = useState(fmtIn(twipsToInches(o.bounds.left)));
  const [topIn, setTopIn] = useState(fmtIn(twipsToInches(o.bounds.top)));
  const [wIn, setWIn] = useState(fmtIn(twipsToInches(o.bounds.width)));
  const [hIn, setHIn] = useState(fmtIn(twipsToInches(o.bounds.height)));
  const [color, setColor] = useState(
    t ? rgbToHex(t.color.r, t.color.g, t.color.b) : "#000000",
  );

  return (
    <>
      <header>
        <div className="text-base font-semibold text-slate-100">{o.name || "(unnamed)"}</div>
        <div className="text-xs text-slate-500">
          {o.kind} · {o.section}
        </div>
      </header>

      {t && (
        <section className="space-y-2">
          <h3 className="text-xs font-semibold tracking-wide text-slate-400 uppercase">Text</h3>
          {t.editable ? (
            <textarea
              className={input}
              rows={3}
              value={text}
              onChange={(e) => setText(e.target.value)}
              onBlur={() => text !== t.text && add({ op: "set_text", object: { id: o.id }, text })}
            />
          ) : (
            <p className="rounded bg-slate-800/60 p-2 text-xs text-slate-400">
              {t.display}
              <br />
              <em>Contains embedded field references — text editing needs multi-run support (not in v1).</em>
            </p>
          )}
          <div className="grid grid-cols-2 gap-2">
            <div>
              <label className={label}>Font</label>
              <input
                className={input}
                value={fontName}
                onChange={(e) => setFontName(e.target.value)}
                onBlur={() =>
                  fontName !== t.font.name &&
                  add({ op: "set_font", object: { id: o.id }, name: fontName })
                }
              />
            </div>
            <div>
              <label className={label}>Size (pt)</label>
              <input
                className={input}
                inputMode="decimal"
                value={sizePt}
                onChange={(e) => setSizePt(e.target.value)}
                onBlur={() => {
                  const pt = parseFloat(sizePt);
                  if (!Number.isNaN(pt) && pt !== t.font.size_pt)
                    add({ op: "set_font", object: { id: o.id }, size_pt: pt });
                }}
              />
            </div>
          </div>
          <div className="flex items-center gap-3 text-xs text-slate-300">
            <Toggle checked={bold} label="Bold" onChange={setBold} onCommit={(v) => v !== t.font.bold && add({ op: "set_font", object: { id: o.id }, bold: v })} />
            <Toggle checked={italic} label="Italic" onChange={setItalic} onCommit={(v) => v !== t.font.italic && add({ op: "set_font", object: { id: o.id }, italic: v })} />
          </div>
          <div>
            <label className={label}>Color</label>
            <input
              type="color"
              className="h-8 w-16 cursor-pointer rounded border border-slate-600 bg-slate-800"
              value={color}
              onChange={(e) => setColor(e.target.value)}
              onBlur={() => {
                const { r, g, b } = hexToRgb(color);
                if (r !== t.color.r || g !== t.color.g || b !== t.color.b)
                  add({ op: "set_text_color", object: { id: o.id }, r, g, b });
              }}
            />
          </div>
        </section>
      )}

      <section className="space-y-2">
        <h3 className="text-xs font-semibold tracking-wide text-slate-400 uppercase">
          Position & size (inches)
        </h3>
        <div className="grid grid-cols-2 gap-2">
          {(
            [
              ["Left", leftIn, setLeftIn, "left"],
              ["Top", topIn, setTopIn, "top"],
              ["Width", wIn, setWIn, "width"],
              ["Height", hIn, setHIn, "height"],
            ] as const
          ).map(([name, val, set, key]) => (
            <div key={key}>
              <label className={label}>{name}</label>
              <input
                className={input}
                inputMode="decimal"
                value={val}
                onChange={(e) => set(e.target.value)}
                onBlur={() => {
                  const inches = parseFloat(val);
                  if (!Number.isNaN(inches)) {
                    const tw = inchesToTwips(inches);
                    if (tw !== o.bounds[key])
                      add({ op: "set_bounds", object: { id: o.id }, [key]: tw });
                  }
                }}
              />
            </div>
          ))}
        </div>
      </section>
    </>
  );
}

function SectionForm({
  area,
  name,
  heightTw,
  add,
}: {
  area: string;
  name: string;
  heightTw: number;
  add: (op: EditOp) => void;
}) {
  const [hPt, setHPt] = useState(fmtPt(twipsToPoints(heightTw)));
  return (
    <>
      <header>
        <div className="text-base font-semibold text-slate-100">{name}</div>
        <div className="text-xs text-slate-500">Section · {area}</div>
      </header>
      <section className="space-y-2">
        <label className={label}>Height (points)</label>
        <input
          className={input}
          inputMode="decimal"
          value={hPt}
          onChange={(e) => setHPt(e.target.value)}
          onBlur={() => {
            const pt = parseFloat(hPt);
            if (!Number.isNaN(pt) && pointsToTwips(pt) !== heightTw)
              add({ op: "set_section_height", section: { area, section: name }, height: pointsToTwips(pt) });
          }}
        />
      </section>
    </>
  );
}

function PageForm({ ir, add }: { ir: Ir; add: (op: EditOp) => void }) {
  const po = ir.print_options;
  const [m, setM] = useState({
    left: fmtIn(twipsToInches(po.margins.left)),
    right: fmtIn(twipsToInches(po.margins.right)),
    top: fmtIn(twipsToInches(po.margins.top)),
    bottom: fmtIn(twipsToInches(po.margins.bottom)),
  });
  const landscape = po.paper_orientation === "Landscape";
  return (
    <>
      <header>
        <div className="text-base font-semibold text-slate-100">Page setup</div>
        <div className="text-xs text-slate-500">{ir.file}</div>
      </header>
      <section className="space-y-2">
        <h3 className="text-xs font-semibold tracking-wide text-slate-400 uppercase">Margins (inches)</h3>
        <div className="grid grid-cols-2 gap-2">
          {(Object.keys(m) as (keyof typeof m)[]).map((k) => (
            <div key={k}>
              <label className={label}>{k}</label>
              <input
                className={input}
                inputMode="decimal"
                value={m[k]}
                onChange={(e) => setM({ ...m, [k]: e.target.value })}
                onBlur={() => {
                  const inches = parseFloat(m[k]);
                  if (!Number.isNaN(inches) && inchesToTwips(inches) !== po.margins[k])
                    add({ op: "set_margins", [k]: inchesToTwips(inches) });
                }}
              />
            </div>
          ))}
        </div>
      </section>
      <section>
        <h3 className="mb-2 text-xs font-semibold tracking-wide text-slate-400 uppercase">Orientation</h3>
        <div className="flex gap-2">
          {(["portrait", "landscape"] as const).map((o) => (
            <button
              key={o}
              onClick={() => (o === "landscape") !== landscape && add({ op: "set_orientation", orientation: o })}
              className={`rounded px-3 py-1 text-xs capitalize ${
                (o === "landscape") === landscape
                  ? "bg-sky-600 text-white"
                  : "border border-slate-600 text-slate-300 hover:bg-slate-700/40"
              }`}
            >
              {o}
            </button>
          ))}
        </div>
        <p className="mt-2 text-[11px] text-slate-500">
          Orientation/paper need the report to carry a page-DEVMODE record (reports saved with a
          printer); otherwise the backend refuses with an explanation.
        </p>
      </section>
    </>
  );
}

function Toggle({
  checked,
  label: text,
  onChange,
  onCommit,
}: {
  checked: boolean;
  label: string;
  onChange: (v: boolean) => void;
  onCommit: (v: boolean) => void;
}) {
  return (
    <label className="flex cursor-pointer items-center gap-1.5">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        onBlur={() => onCommit(checked)}
        className="h-3.5 w-3.5 accent-sky-500"
      />
      {text}
    </label>
  );
}

/* Two ops target the same field when op + object + field key all match. */
function sameSlot(a: EditOp, b: EditOp): boolean {
  if (a.op !== b.op) return false;
  const oa = "object" in a ? JSON.stringify(a.object) : "section" in a ? JSON.stringify(a.section) : "";
  const ob = "object" in b ? JSON.stringify(b.object) : "section" in b ? JSON.stringify(b.section) : "";
  if (oa !== ob) return false;
  const keys = ["left", "top", "width", "height", "text", "name", "size_pt", "bold", "italic", "underline", "strikeout", "weight"];
  return keys.every((k) => (k in a ? (a as never)[k] : undefined) === (k in b ? (b as never)[k] : undefined));
}

const fmtIn = (v: number): string => String(Math.round(v * 1000) / 1000);
const fmtPt = (v: number): string => String(Math.round(v * 10) / 10);
const rgbToHex = (r: number, g: number, b: number): string =>
  "#" + [r, g, b].map((v) => v.toString(16).padStart(2, "0")).join("");
const hexToRgb = (hex: string): { r: number; g: number; b: number } => {
  const n = parseInt(hex.slice(1), 16);
  return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
};
