import { useId, useMemo } from "react";
import type { Effect, Flow, Here, SavedTemplate } from "../../bindings";
import { PanZoomControls, PanZoomViewport, usePanZoom } from "../../components/PanZoom";
import { Button } from "../../components/ui/button";
import { IconOpenInBrowser, IconRemove } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { GEOMETRY, edgePath, layoutFlow, type Placed } from "../../lib/flowLayout";
import HereBadge, { HereDot, failedHint, failureLine } from "./HereBadge";
import { EffectBadge, dayMonth, stampDate } from "./TemplateRow";

/** The arrow's head at `to`'s left-middle; edges arrive horizontally. */
function arrowHead(to: Placed): string {
  const x = to.x;
  const y = to.y + to.h / 2;
  return `M${x - 6} ${y - 4} L${x} ${y} L${x - 6} ${y + 4}`;
}

/** A stage's tone: what it does. One effect across its templates takes that
 *  effect's token; no template yet, or templates that disagree, take the
 *  theme's accent. The tone is a 3px left edge on the stage; the arrows
 *  between stages are all muted. */
export type EdgeTone = Effect | "none";

const STAGE_EDGE: Record<EdgeTone, string> = {
  create: "border-l-success",
  edit: "border-l-warning",
  delete: "border-l-danger",
  read: "border-l-muted",
  none: "border-l-accent",
};

/** The legend's swatch for each tone, and its words. */
const LEGEND: Array<{ tone: EdgeTone; swatch: string; label: string }> = [
  { tone: "create", swatch: "bg-success", label: "Create" },
  { tone: "edit", swatch: "bg-warning", label: "Edit" },
  { tone: "delete", swatch: "bg-danger", label: "Delete" },
  { tone: "read", swatch: "bg-muted", label: "Read" },
  { tone: "none", swatch: "bg-accent", label: "Open: no template yet, or mixed" },
];

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
 * The drawing pans and zooms like the Test map (PanZoom): drag, wheel,
 * Ctrl + wheel, the header's zoom controls, or the keys once it has focus.
 */
export default function FlowMap({
  flow,
  here,
  templates,
  onOpenTemplate,
  onView,
  onRemove,
}: {
  flow: Flow;
  /** Whether the flow has worked on the active environment. */
  here: Here;
  /** The saved templates whose `stage` names this flow. */
  templates: SavedTemplate[];
  onOpenTemplate: (id: string) => void;
  /** Open the flow on its own page in the browser. */
  onView: () => void;
  onRemove: () => void;
}) {
  const headingId = useId();

  const byStage = useMemo(() => {
    const m = new Map<string, SavedTemplate[]>();
    for (const saved of templates) {
      const t = saved.template;
      if (!t.stage) continue;
      const list = m.get(t.stage.id) ?? [];
      list.push(saved);
      m.set(t.stage.id, list);
    }
    for (const list of m.values()) list.sort((a, b) => a.template.title.localeCompare(b.template.title));
    return m;
  }, [templates]);

  const layout = useMemo(() => {
    const counts: Record<string, number> = {};
    const failures: Record<string, number> = {};
    for (const [id, list] of byStage) {
      counts[id] = list.length;
      failures[id] = list.filter((s) => s.here.state === "failed" && failureLine(s.here.failure)).length;
    }
    return layoutFlow(flow, counts, failures);
  }, [flow, byStage]);

  const pz = usePanZoom({ w: layout.width, h: layout.height });

  const placed = new Map(layout.boxes.map((b) => [b.id, b]));
  const titleOf = new Map(flow.stages.map((s) => [s.id, s.title]));
  const stageTone = new Map(
    flow.stages.map((s) => [s.id, edgeTone((byStage.get(s.id) ?? []).map((x) => x.template))] as const),
  );
  // The legend names only the tones this flow shows.
  const legend = LEGEND.filter((l) => flow.stages.some((s) => stageTone.get(s.id) === l.tone));
  const saved = flow.saved ? stampDate(flow.saved.at) : null;

  return (
    <section aria-labelledby={headingId} className="space-y-3 rounded-md border border-border bg-surface p-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h3 id={headingId} className="text-sm font-semibold text-text">
          {flow.title}
        </h3>
        <HereBadge here={here} />
        <span className="text-xs text-muted">{flow.module}</span>
        <span className="text-xs text-muted">Tracks {flow.subject.name}</span>
        {flow.saved && (
          <span className="text-xs text-faint">Saved {saved ? dayMonth(saved) : flow.saved.at}</span>
        )}
        <PanZoomControls pz={pz} className="ml-auto" />
        <Button
          size="sm"
          variant="ghost"
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

      <PanZoomViewport pz={pz} label={`${flow.title} map`} testId="flow-map">
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
            return (
              <g
                key={`${from}->${to}`}
                className="flow-edge text-muted"
                stroke="currentColor"
                strokeWidth={1.75}
                strokeLinecap="round"
                strokeLinejoin="round"
              >
                <path data-edge d={edgePath(a, b)} />
                <path d={arrowHead(b)} />
                {/* A short darker stretch running from the arrow's start to
                    its end, only where motion is welcome. `pathLength` makes
                    the run the same pace on a long arrow as a short one. */}
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
          const tone = stageTone.get(s.id) ?? "none";
          return (
            <div
              key={s.id}
              role="group"
              aria-label={s.title}
              data-tone={tone}
              className={cn(
                "absolute flex flex-col rounded-md border border-l-[3px] bg-surface-2 px-2",
                // The all-sides colour first: tailwind-merge drops a left-edge
                // colour that comes before it.
                s.optional ? "border-dashed border-border-strong [border-left-style:solid]" : "border-border",
                STAGE_EDGE[tone],
              )}
              style={{ left: box.x, top: box.y, width: GEOMETRY.colW, height: box.h }}
            >
              <div className="flex h-10 shrink-0 items-center gap-2">
                <span title={s.title} className="min-w-0 flex-1 line-clamp-2 break-words text-xs font-semibold leading-4 text-text">
                  {s.title}
                </span>
                {s.optional && <span className="text-[10px] text-faint">Optional</span>}
              </div>
              {list.length === 0 ? (
                <p className="flex h-6 items-center text-xs text-faint">No template yet</p>
              ) : (
                <ul>
                  {list.map(({ template: t, here: tHere }) => {
                    const why = tHere.state === "failed" ? failureLine(tHere.failure) : null;
                    return (
                      <li key={t.id}>
                        <button
                          title={`Show ${t.title} below`}
                          className="flex h-6 w-full min-w-0 items-center gap-1.5 rounded px-1 text-left text-xs text-text transition-colors hover:bg-surface hover:text-accent"
                          onClick={() => onOpenTemplate(t.id)}
                        >
                          <span className="min-w-0 flex-1 truncate">{t.title}</span>
                          <HereDot here={tHere} />
                          <EffectBadge effect={t.effect} />
                        </button>
                        {/* The reason is on the map, not only in the dot's tooltip. */}
                        {why && tHere.state === "failed" && (
                          <p
                            title={failedHint(tHere.failure)}
                            className="line-clamp-2 h-8 break-words px-1 text-[11px] leading-4 text-muted"
                          >
                            {why}
                          </p>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          );
        })}
      </PanZoomViewport>

      <ul aria-label="Legend" className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted">
        {legend.map((l) => (
          <li key={l.tone} className="flex items-center gap-2">
            <span aria-hidden className={cn("h-4 w-[3px] rounded-sm", l.swatch)} />
            {l.label}
          </li>
        ))}
        {flow.stages.some((s) => s.optional) && (
          <li className="flex items-center gap-2">
            <span aria-hidden className="size-4 rounded-sm border border-dashed border-border-strong" />
            Optional stage
          </li>
        )}
      </ul>

      <ol aria-label={`Stages of ${flow.title}`} className="sr-only">
        {flow.stages.map((s) => {
          const requires = (s.requires ?? []).map((r) => titleOf.get(r) ?? r);
          const performers = (byStage.get(s.id) ?? []).map(({ template: t, here: tHere }) => {
            if (tHere.state === "confirmed") return t.title;
            if (tHere.state === "unconfirmed") return `${t.title} (unconfirmed here)`;
            const why = failureLine(tHere.failure);
            return `${t.title} (failed here${why ? `, ${why}` : ""})`;
          });
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
