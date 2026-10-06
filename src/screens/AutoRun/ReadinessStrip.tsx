// The one line the Test cases tab opens with: where runs go, and whether the
// setup a run needs is in place. It replaces the header line that sat above
// the tabs. Every item says what it is about in words - the tick or the
// warning beside it is the glance, never the whole message.
//
// A count that has not been read yet (or could not be) is left out rather
// than guessed: a warning that turns out to be a slow read is worse than
// no item for a moment.

import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "../../components/ui/button";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** One thing that needs attention: a warning, then the words. */
function Item({
  title,
  children,
}: {
  title?: string;
  children: ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-1">
      <TriangleAlert aria-hidden className="size-3.5 text-warning" />
      <span className="text-warning" title={title}>
        {children}
      </span>
    </span>
  );
}

export default function ReadinessStrip({
  envName,
  siteHost,
  signIn,
  accountCount,
  missingTestFiles,
  unreadable,
  onOpenSetup,
}: {
  /** The active environment's name, or null when there is none. */
  envName: string | null;
  /** The host runs go to, null when no site address is set, or undefined
   * when it could not be told (a read failed - see `unreadable`). */
  siteHost: string | null | undefined;
  /** A saved recipe, the built-in one, no way to sign in, or not read yet. */
  signIn: "saved" | "builtin" | "none" | null;
  accountCount: number | null;
  /** Files a saved script uploads that the Test files folder does not hold. */
  missingTestFiles: string[];
  /** Reads that failed, each as the sentence its Setup row says. A count
   * that could not be read is left out above; this is where it is said. */
  unreadable: string[];
  onOpenSetup: () => void;
}) {
  // The Setup panel shows what is in place; this line keeps only the
  // environment and whatever needs attention, so a gap stays visible with
  // the panel shut.
  const attention =
    siteHost === null ||
    signIn === "none" ||
    accountCount === 0 ||
    missingTestFiles.length > 0 ||
    unreadable.length > 0;
  return (
    <div
      role="group"
      aria-label="Readiness"
      className="flex items-center gap-3 rounded-md border border-border bg-surface px-3 py-1.5 text-xs text-muted"
    >
      {/* The items wrap among themselves; Open setup stays at the end of
          the first line instead of dropping onto a line of its own. */}
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-4 gap-y-2">
      <span className="inline-flex items-center gap-1">
        <span>{envName ? "Environment" : "Runs against"} </span>
        {siteHost === null ? (
          // A run cannot go without an address, so it is flagged like the
          // other gaps.
          <>
            {envName && <span className="font-medium text-text">{envName} - </span>}
            <Item>no site set yet</Item>
          </>
        ) : (
          <span className="font-medium text-text">
            {envName && `${envName} - `}
            {siteHost ?? "not known"}
          </span>
        )}
      </span>

      {signIn === "none" && <Item>No sign-in</Item>}

      {accountCount === 0 && <Item>No accounts</Item>}

      {missingTestFiles.length > 0 && (
        <Item title={`Not in the Test files folder: ${missingTestFiles.join(", ")}`}>
          {plural(missingTestFiles.length, "test file")} missing
        </Item>
      )}

      {unreadable.map((sentence) => (
        <Item key={sentence}>
          {sentence}
        </Item>
      ))}

      </div>

      {attention && (
        <Button size="sm" variant="ghost" className="shrink-0" onClick={onOpenSetup}>
          Open setup
        </Button>
      )}
    </div>
  );
}
