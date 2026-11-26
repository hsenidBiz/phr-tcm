import type { Here, HereFailure } from "../../bindings";
import { Badge } from "../../components/ui/badge";

/** Said wherever an item that has not worked on this environment yet is
 * marked. */
export const UNCONFIRMED_HINT =
  "Not confirmed on this environment yet. Ask the assistant to try it here before relying on it.";

/** What the Failed here mark says: where it failed and why, when known. */
export function failedHint(f: HereFailure): string {
  return `Failed on this environment${f.step ? ` at ${f.step}` : ""}${f.detail ? `: ${f.detail}` : ""}. Ask the assistant to make it work here.`;
}

/** The short reason a failure shows on the page itself, the second line
 * under a template: "failed at step 3: HTTP 500". The tooltip keeps the full
 * sentence; null when the failure names neither a step nor a reason. Worded
 * as the View flow page words it (`flow_page.rs`). */
export function failureLine(f: HereFailure): string | null {
  const at = f.step ? (/^\d+$/.test(f.step) ? `step ${f.step}` : f.step) : null;
  let detail = f.detail ? f.detail.split(/\s+/).filter(Boolean).join(" ") : null;
  if (detail && [...detail].length > 120) detail = `${[...detail].slice(0, 119).join("").trimEnd()}\u2026`;
  if (at && detail) return `failed at ${at}: ${detail}`;
  if (at) return `failed at ${at}`;
  if (detail) return `failed: ${detail}`;
  return null;
}

/**
 * Whether a template, flow or fixture has worked on the active environment.
 * A confirmed one carries no mark: only what needs a look is flagged.
 */
export default function HereBadge({ here }: { here: Here }) {
  if (here.state === "confirmed") return null;
  if (here.state === "unconfirmed") {
    return (
      <Badge className="bg-warning/15 text-warning" title={UNCONFIRMED_HINT}>
        Unconfirmed here
      </Badge>
    );
  }
  return (
    <Badge className="bg-danger/15 text-danger" title={failedHint(here.failure)}>
      Failed here
    </Badge>
  );
}

/**
 * The same state as a small dot, for where a worded badge would take the
 * room of a title (a template inside a flow-map stage). Its words are its
 * tooltip and its accessible name; a confirmed item shows nothing.
 */
export function HereDot({ here }: { here: Here }) {
  if (here.state === "confirmed") return null;
  const failed = here.state === "failed";
  const label = failed ? "Failed here" : "Unconfirmed here";
  return (
    <span
      role="img"
      aria-label={label}
      title={failed ? failedHint(here.failure) : UNCONFIRMED_HINT}
      className={`size-2 shrink-0 rounded-full ${failed ? "bg-danger" : "bg-warning"}`}
    />
  );
}
