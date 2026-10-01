import { ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";
import type { ApiTemplateStep, Effect, RunRecord, SavedTemplate } from "../../bindings";
import { Badge } from "../../components/ui/badge";
import { Button } from "../../components/ui/button";
import { Collapse } from "../../components/ui/collapse";
import { IconRemove } from "../../lib/actionIcons";
import { cn } from "../../lib/cn";
import { relativeTime } from "../../lib/history";

/** The effect badge's tokens: what a run does to the application's data. */
export const EFFECT_TONE: Record<Effect, string> = {
  create: "text-success bg-success/15",
  edit: "text-warning bg-warning/15",
  delete: "text-danger bg-danger/15",
};

/** What a run does to the application's data, as the list and the flow map
 * both show it. */
export function EffectBadge({ effect }: { effect: Effect }) {
  return <Badge className={EFFECT_TONE[effect]}>{effect}</Badge>;
}

/** A proving or run stamp - "YYYY-MM-DD HH:MM:SS" in UTC, the app log's
 * own format - as a Date, or null when it is not one. */
export function stampDate(at: string): Date | null {
  const d = new Date(`${at.replace(" ", "T")}Z`);
  return Number.isNaN(d.getTime()) ? null : d;
}

/** Spelled out here rather than asked of the locale, whose en-GB short
 * month for September is "Sept" on some ICU builds and "Sep" on others. */
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/** "28 Sep", in this machine's time. */
export function dayMonth(d: Date): string {
  return `${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

/** "28 Sep, 16:05", in this machine's time. */
function whenLong(at: string): string {
  const d = stampDate(at);
  if (!d) return at;
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${dayMonth(d)}, ${hh}:${mm}`;
}

/** Captured values as "name: value" lines. They arrive as whatever JSON the
 * application answered with, so anything that is not an object shows
 * nothing rather than a guess. */
export function outputPairs(v: unknown): string[] {
  if (!v || typeof v !== "object" || Array.isArray(v)) return [];
  return Object.entries(v as Record<string, unknown>).map(([k, x]) => `${k}: ${JSON.stringify(x)}`);
}

/** The path as the application sees it, handler and all. */
function fullPath(step: ApiTemplateStep): string {
  const q = Object.entries(step.query ?? {});
  if (q.length === 0) return step.path;
  const sep = step.path.includes("?") ? "&" : "?";
  return `${step.path}${sep}${q.map(([k, v]) => `${k}=${v}`).join("&")}`;
}

/** The body's field names - the shape, not the values, which are mostly
 * placeholders anyway. */
function bodyFields(step: ApiTemplateStep): string | null {
  if (step.form && Object.keys(step.form).length > 0) return Object.keys(step.form).join(", ");
  const json = step.json;
  if (json && typeof json === "object" && !Array.isArray(json)) {
    const keys = Object.keys(json as Record<string, unknown>);
    return keys.length > 0 ? keys.join(", ") : null;
  }
  if (Array.isArray(json)) return "a list";
  return null;
}

function expectText(step: ApiTemplateStep): string | null {
  const parts: string[] = [];
  if (step.expect?.status != null) parts.push(`status ${step.expect.status}`);
  const json = step.expect?.json;
  if (json && typeof json === "object" && !Array.isArray(json)) {
    parts.push(...outputPairs(json));
  }
  return parts.length > 0 ? parts.join(", ") : null;
}

/** The test files a form step uploads: each field and the file's name. */
function filesText(step: ApiTemplateStep): string | null {
  const f = Object.entries(step.files ?? {});
  return f.length > 0 ? f.map(([field, name]) => `${field} ← ${name}`).join(", ") : null;
}

function captureText(step: ApiTemplateStep): string | null {
  const c = Object.entries(step.capture ?? {});
  return c.length > 0 ? c.map(([name, path]) => `${name} ← ${path}`).join(", ") : null;
}

/** "hrmmainphdev01.phrsandbox.dev" from the recipe's origin. */
export function hostOf(origin: string): string {
  try {
    return new URL(origin).host;
  } catch {
    return origin;
  }
}

/** A labelled line in a step or a run; absent when there is nothing to say. */
function Detail({ label, value }: { label: string; value: string | null }) {
  if (!value) return null;
  return (
    <div className="flex gap-2">
      <span className="w-16 shrink-0 text-faint">{label}</span>
      <span className="id-mono min-w-0 break-all text-muted">{value}</span>
    </div>
  );
}

/** What a history line was. A history written before lines said so held
 * runs only, so a line without a mode is a run. */
export function runMode(r: RunRecord): "prove" | "run" {
  return r.mode === "prove" ? "prove" : "run";
}

/** Said wherever a template with no proof is shown: only an import saves
 * one without it, and runs of it are allowed. */
export const UNPROVEN_HINT =
  "Imported - not proven on this site. Prove it here before relying on it; its run history may be from the version it replaced.";

function LastRun({ run }: { run: RunRecord | undefined }) {
  if (!run) return <span className="text-xs text-faint">never run</span>;
  const d = stampDate(run.at);
  return (
    <span className="flex items-center gap-1.5 text-xs text-muted" title={whenLong(run.at)}>
      <span aria-hidden className={cn("size-2 rounded-full", run.ok ? "bg-success" : "bg-danger")} />
      <span className="sr-only">{run.ok ? "succeeded" : "failed"}</span>
      last run {d ? relativeTime(d.toISOString()) : run.at}
    </span>
  );
}

function RunLine({ run }: { run: RunRecord }) {
  const outputs = outputPairs(run.outputs);
  return (
    <li className="flex flex-col gap-0.5 rounded border border-border px-2 py-1.5">
      <div className="flex items-center gap-2">
        <span aria-hidden className={cn("size-2 shrink-0 rounded-full", run.ok ? "bg-success" : "bg-danger")} />
        <span className="text-text">{whenLong(run.at)}</span>
        <span className="text-faint">{runMode(run)}</span>
        <span className="text-muted">{run.account}</span>
        <span className={run.ok ? "text-success" : "text-danger"}>
          {run.ok ? "ok" : run.failed_step ? `failed at ${run.failed_step}` : "failed"}
        </span>
      </div>
      {run.detail && <p className="break-words text-muted">{run.detail}</p>}
      {outputs.length > 0 && <p className="id-mono break-all text-faint">{outputs.join(", ")}</p>}
    </li>
  );
}

/**
 * One saved template: a line to scan, and - opened - everything the
 * assistant wrote and proved, read-only. The only thing a person can do to
 * a template here is remove it (spec §8: no edit, no run, no duplicate).
 *
 * Open or folded is its own business unless the tab passes `open` - which
 * it does so that clicking the template on a flow map can open it.
 */
/** A template's stage, as its row shows it. */
export type StageLine = { stage: string; flow: string; missing: boolean };

export default function TemplateRow({
  saved,
  onRemove,
  open: openProp,
  onOpenChange,
  stage,
}: {
  saved: SavedTemplate;
  onRemove: () => void;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  /** The flow stage it performs - the stage's title on the row, and which
   * flow it is in when opened; `missing` when that flow or stage is no
   * longer saved, so every run of it is refused. */
  stage?: StageLine;
}) {
  const [ownOpen, setOwnOpen] = useState(false);
  const open = openProp ?? ownOpen;
  const setOpen = (next: boolean) => {
    if (openProp === undefined) setOwnOpen(next);
    onOpenChange?.(next);
  };
  const t = saved.template;
  const n = t.params.length;
  const provenOutputs = outputPairs(t.proven?.outputs);
  // The prove that saved it is history, not a run: "last run" is a run's.
  const lastRun = saved.runs.find((r) => runMode(r) === "run");

  return (
    <li id={`api-template-${t.id}`} aria-label={t.title} className="rounded-md border border-border bg-surface">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2">
        <button
          aria-label={`${open ? "Hide" : "Show"} details of ${t.title}`}
          aria-expanded={open}
          title={open ? "Hide details" : "Show details"}
          className="text-muted transition-colors hover:text-accent"
          onClick={() => setOpen(!open)}
        >
          {open ? <ChevronDown size={15} /> : <ChevronRight size={15} />}
        </button>
        {/* The line to scan: what it is, what it does, where it sits in its
            flow, and when it last ran. Its parameters and proof are one
            click away, in the details. */}
        <span className="min-w-0 flex-1 text-sm font-medium break-words text-text">{t.title}</span>
        {/* Before the effect and the last run, so a run of the version an
            import replaced is never read as this one's proof. */}
        {!t.proven && (
          <Badge className="bg-warning/15 text-warning" title={UNPROVEN_HINT}>
            Unproven
          </Badge>
        )}
        <EffectBadge effect={t.effect} />
        {stage && (
          <span className={cn("text-xs", stage.missing ? "text-warning" : "text-muted")}>
            {`Stage: ${stage.stage}${stage.missing ? " (no longer saved)" : ""}`}
          </span>
        )}
        <LastRun run={lastRun} />
        <Button size="sm" variant="ghost" aria-label={`Remove ${t.title}`} onClick={onRemove}>
          <IconRemove aria-hidden />
          Remove
        </Button>
      </div>
      <Collapse open={open}>
        <div data-testid="template-details" className="space-y-4 border-t border-border px-3 py-3 text-xs">
          <div className="space-y-1">
            <p className="text-text">{t.description}</p>
            <p className="id-mono text-faint">{t.id}</p>
          </div>

          {stage && (
            <section className="space-y-1">
              <h3 className="font-semibold text-muted">Flow</h3>
              <p data-testid="template-flow" className={stage.missing ? "text-warning" : "text-muted"}>
                {stage.missing
                  ? `${stage.stage} in ${stage.flow} - no longer saved, so every run of this template is refused until a flow with this stage is saved again.`
                  : `${stage.stage}, in ${stage.flow}`}
              </p>
            </section>
          )}

          <section className="space-y-1">
            <h3 className="font-semibold text-muted">{`Parameters (${n})`}</h3>
            {n === 0 ? (
              <p className="text-faint">None.</p>
            ) : (
              <table aria-label="Parameters" className="w-full text-left">
                <thead className="text-faint">
                  <tr>
                    <th className="py-1 pr-3 font-medium">Name</th>
                    <th className="py-1 pr-3 font-medium">Type</th>
                    <th className="py-1 pr-3 font-medium">Required</th>
                    <th className="py-1 pr-3 font-medium">Default</th>
                    <th className="py-1 pr-3 font-medium">Description</th>
                    <th className="py-1 font-medium">Lookup hint</th>
                  </tr>
                </thead>
                <tbody>
                  {t.params.map((p) => (
                    <tr key={p.name} className="border-t border-border align-top">
                      <td className="id-mono py-1 pr-3 text-text">{p.name}</td>
                      <td className="py-1 pr-3 text-muted">{p.type}</td>
                      <td className="py-1 pr-3 text-muted">{p.required ? "yes" : "no"}</td>
                      {/* What a run that leaves an optional param out sends. */}
                      <td className="id-mono break-all py-1 pr-3 text-muted">
                        {p.default === undefined || p.default === null ? "" : JSON.stringify(p.default)}
                      </td>
                      <td className="py-1 pr-3 text-muted">{p.description ?? ""}</td>
                      <td className="id-mono break-all py-1 text-faint">{p.lookup ?? ""}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className="space-y-1">
            <h3 className="font-semibold text-muted">Steps</h3>
            <ol aria-label="Steps" className="space-y-1.5">
              {t.steps.map((s, i) => (
                <li key={`${i}-${s.name}`} className="space-y-0.5 rounded border border-border px-2 py-1.5">
                  <div className="flex items-baseline gap-2">
                    <span className="text-faint">{i + 1}.</span>
                    <span className="font-semibold text-text">{s.method}</span>
                    <span className="id-mono min-w-0 break-all text-text">{fullPath(s)}</span>
                    <span className="text-faint">{s.name}</span>
                  </div>
                  <Detail label="Body" value={bodyFields(s)} />
                  <Detail label="Files" value={filesText(s)} />
                  <Detail label="Expects" value={expectText(s)} />
                  <Detail label="Captures" value={captureText(s)} />
                </li>
              ))}
            </ol>
          </section>

          {t.sources.length > 0 && (
            <section className="space-y-1">
              <h3 className="font-semibold text-muted">Sources</h3>
              <ul aria-label="Sources" className="space-y-0.5">
                {t.sources.map((src) => (
                  <li key={src} className="id-mono break-all text-muted">
                    {src}
                  </li>
                ))}
              </ul>
            </section>
          )}

          {!t.proven && (
            <section className="space-y-1">
              <h3 className="font-semibold text-muted">Not proven</h3>
              <p data-testid="template-proof" className="text-warning">
                {UNPROVEN_HINT}
              </p>
            </section>
          )}

          {t.proven && (
            <section className="space-y-1">
              <h3 className="font-semibold text-muted">Proven</h3>
              <div data-testid="template-proof" className="space-y-0.5 text-muted">
                <p>
                  {whenLong(t.proven.at)} as {t.proven.account} against {hostOf(t.proven.origin)}
                </p>
                {provenOutputs.length > 0 && (
                  <p className="id-mono break-all text-faint">Created {provenOutputs.join(", ")}</p>
                )}
              </div>
            </section>
          )}

          <section className="space-y-1">
            <h3 className="font-semibold text-muted">Runs</h3>
            {!lastRun && <p className="text-faint">{t.proven ? "Not run since it was proven." : "Not run yet."}</p>}
            {!t.proven && saved.runs.length > 0 && (
              <p className="text-warning">These lines may be from the version an import replaced, not this one.</p>
            )}
            {saved.runs.length > 0 && (
              <ol aria-label="Runs" className="space-y-1">
                {saved.runs.map((r, i) => (
                  <RunLine key={`${r.at}-${i}`} run={r} />
                ))}
              </ol>
            )}
          </section>
        </div>
      </Collapse>
    </li>
  );
}
