import { useId, useMemo } from "react";
import type { Effect, Flow, SavedTemplate } from "../../bindings";
import { Button } from "../../components/ui/button";
import { IconOpenInBrowser, IconRemove } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { GEOMETRY, edgePath, layoutFlow, type Placed } from "../../lib/flowLayout";
import { EffectBadge, dayMonth, stampDate } from "./TemplateRow";

/** The arrow's head at `to`'s left-middle; edges arrive horizontally. */
function arrowHead(to: Placed): string {
  const x = to.x;
  const y = to.y + to.h / 2;
  return `M${x - 6} ${y - 4} L${x} ${y} L${x - 6} ${y + 4}`;
}

/** An arrow's colour: what the stage it leads into does. One effect across
 *  its templates takes that effect's token; no template yet, or templates
 *  that disagree, take the theme's accent. */
export type EdgeTone = Effect | "none";

const EDGE_TONE: Record<EdgeTone, string> = {
  create: "text-success",
  edit: "text-warning",
  delete: "text-danger",
  none: "text-accent",
};

export function edgeTone(templates: { effect: Effect }[]): EdgeTone {
  const effects = new Set(templates.map((t) => t.effect));
  return effects.size === 1 ? [...effects][0] : "none";
}

/** Seconds between one column's pulse and the next, so the pulse runs
 *  through the flow left to right rather than along every arrow at once. */
const PULSE_STAGGER = 0.18;

/**
 * One flow, drawn as a map (spec §8): its stages left to right by depth,
 * arrows from each required stage to the stage that requires it, optional
 * stages dashed, and on each stage the templates that perform it - or
 * "No template yet". Clicking a template opens its row in the list below.
 *
 * Positions come from `layoutFlow`, never from measuring: the arrows are an
 * `aria-hidden` SVG, and a visually hidden list says the same in words.
 */
export default function FlowMap({
  flow,
  templates,
  onOpenTemplate,
  onView,
  onRemove,
}: {
  flow: Flow;
  /** The saved templates whose `stage` names this flow. */
  templates: SavedTemplate[];
  onOpenTemplate: (id: string) => void;
  /** Open the flow on its own page in the browser. */
  onView: () => void;
  onRemove: () => void;
}) {
  const headingId = useId();

  const byStage = useMemo(() => {
    const m = new Map<string, SavedTemplate["template"][]>();
    for (const { template: t } of templates) {
      if (!t.stage) continue;
      const list = m.get(t.stage.id) ?? [];
      list.push(t);
      m.set(t.stage.id, list);
    }
    for (const list of m.values()) list.sort((a, b) => a.title.localeCompare(b.title));
    return m;
  }, [templates]);

  const layout = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const [id, list] of byStage) counts[id] = list.length;
    return layoutFlow(flow, counts);
  }, [flow, byStage]);

  const placed = new Map(layout.boxes.map((b) => [b.id, b]));
  const titleOf = new Map(flow.stages.map((s) => [s.id, s.title]));
  const saved = flow.saved ? stampDate(flow.saved.at) : null;

  return (
    <section aria-labelledby={headingId} className="space-y-3 rounded-md border border-border bg-surface p-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h3 id={headingId} className="text-sm font-semibold text-text">
          {flow.title}
        </h3>
        <span className="text-xs text-muted">{flow.module}</span>
        <span className="text-xs text-muted">Tracks {flow.subject.name}</span>
        {flow.saved && (
          <span className="text-xs text-faint">Saved {saved ? dayMonth(saved) : flow.saved.at}</span>
        )}
        <Button
          size="sm"
          variant="ghost"
          className="ml-auto"
          aria-label={`View flow ${flow.title} in the browser`}
          onClick={onView}
        >
          <IconOpenInBrowser aria-hidden />
          View flow
        </Button>
        <Button
          size="sm"
          variant="ghost"
          aria-label={`Remove flow ${flow.title}`}
          onClick={onRemove}
        >
          <IconRemove aria-hidden />
          Remove flow
        </Button>
      </div>

      <div data-testid="flow-map" className="overflow-x-auto pb-1">
        <div className="relative" style={{ width: layout.width, height: layout.height }}>
          <svg
            aria-hidden="true"
            width={layout.width}
            height={layout.height}
            className="absolute inset-0 fill-none"
          >
            {layout.edges.map(({ from, to }) => {
              const a = placed.get(from);
              const b = placed.get(to);
              if (!a || !b) return null;
              const tone = edgeTone(byStage.get(to) ?? []);
              return (
                <g
                  key={`${from}->${to}`}
                  data-tone={tone}
                  className={cn("flow-edge", EDGE_TONE[tone])}
                  stroke="currentColor"
                  strokeWidth={1.75}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                >
                  <path data-edge d={edgePath(a, b)} />
                  <path d={arrowHead(b)} />
                  {/* A short bright stretch running from the arrow's start to
                      its end, as the Test map's limbs pulse when a case is
                      hovered. `pathLength` makes the run the same pace on a
                      long arrow as a short one. */}
                  <path
                    className="flow-pulse"
                    d={edgePath(a, b)}
                    pathLength={1}
                    style={{ animationDelay: `${a.col * PULSE_STAGGER}s` }}
                  />
                </g>
              );
            })}
          </svg>
          {flow.stages.map((s) => {
            const box = placed.get(s.id);
            if (!box) return null;
            const list = byStage.get(s.id) ?? [];
            return (
              <div
                key={s.id}
                role="group"
                aria-label={s.title}
                className={cn(
                  "absolute flex flex-col rounded-md border bg-surface-2 px-2",
                  s.optional ? "border-dashed border-border-strong" : "border-border",
                )}
                style={{ left: box.x, top: box.y, width: GEOMETRY.colW, height: box.h }}
              >
                <div className="flex h-10 shrink-0 items-center gap-2">
                  <span title={s.title} className="min-w-0 flex-1 truncate text-xs font-semibold text-text">
                    {s.title}
                  </span>
                  {s.optional && <span className="text-[10px] text-faint">Optional</span>}
                </div>
                {list.length === 0 ? (
                  <p className="flex h-6 items-center text-xs text-faint">No template yet</p>
                ) : (
                  <ul>
                    {list.map((t) => (
                      <li key={t.id}>
                        <button
                          title={`Show ${t.title} below`}
                          className="flex h-6 w-full min-w-0 items-center gap-1.5 rounded px-1 text-left text-xs text-text transition-colors hover:bg-surface hover:text-accent"
                          onClick={() => onOpenTemplate(t.id)}
                        >
                          <span className="min-w-0 flex-1 truncate">{t.title}</span>
                          <EffectBadge effect={t.effect} />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            );
          })}
        </div>
      </div>

      <ol aria-label={`Stages of ${flow.title}`} className="sr-only">
        {flow.stages.map((s) => {
          const requires = (s.requires ?? []).map((r) => titleOf.get(r) ?? r);
          const performers = (byStage.get(s.id) ?? []).map((t) => t.title);
          return (
            <li key={s.id}>
              {`${s.title}. Requires: ${requires.length > 0 ? requires.join(", ") : "nothing"}. ${
                s.optional ? "Optional. " : ""
              }Templates: ${performers.length > 0 ? performers.join(", ") : "none yet"}.`}
            </li>
          );
        })}
      </ol>
    </section>
  );
}
