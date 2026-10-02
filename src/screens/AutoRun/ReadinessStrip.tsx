// The one line the Test cases tab opens with: where runs go, and whether the
// setup a run needs is in place. It replaces the header line that sat above
// the tabs. Every item says what it is about in words - the tick or the
// warning beside it is the glance, never the whole message.
//
// A count that has not been read yet (or could not be) is left out rather
// than guessed: a warning that turns out to be a slow read is worse than
// no item for a moment.

import { Check, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "../../components/ui/button";
import { cn } from "../../lib/cn";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** One finding: a tick or a warning, then the words. */
function Item({
  tone,
  title,
  children,
}: {
  tone: "ok" | "warn" | "quiet";
  title?: string;
  children: ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-1">
      {tone === "ok" && <Check aria-hidden className="size-3.5 text-success" />}
      {tone === "warn" && <TriangleAlert aria-hidden className="size-3.5 text-warning" />}
      <span className={cn(tone === "warn" && "text-warning")} title={title}>
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
  areaCount,
  testFileCount,
  missingTestFiles,
  onOpenSetup,
}: {
  /** The active environment's name, or null when there is none. */
  envName: string | null;
  /** The host runs go to, or null when no site address is set. */
  siteHost: string | null;
  /** A saved recipe, the built-in one, no way to sign in, or not read yet. */
  signIn: "saved" | "builtin" | "none" | null;
  accountCount: number | null;
  areaCount: number | null;
  testFileCount: number | null;
  /** Files a saved script uploads that the Test files folder does not hold. */
  missingTestFiles: string[];
  onOpenSetup: () => void;
}) {
  return (
    <div
      role="group"
      aria-label="Readiness"
      className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-md border border-border bg-surface px-3 py-2 text-xs text-muted"
    >
      <span>
        {envName ? "Environment" : "Runs against"}{" "}
        <span className="font-medium text-text">
          {envName && `${envName} - `}
          {siteHost ?? "no site set yet"}
        </span>
      </span>

      {signIn === "none" ? (
        <Item tone="warn">No sign-in</Item>
      ) : signIn ? (
        <Item tone="ok">
          Sign-in <span className="font-medium text-text">{signIn === "builtin" ? "Built-in" : "Saved"}</span>
        </Item>
      ) : null}

      {accountCount != null &&
        (accountCount === 0 ? (
          <Item tone="warn">No accounts</Item>
        ) : (
          <Item tone="ok">{plural(accountCount, "account")}</Item>
        ))}

      {/* Areas help a run find its way but are not needed to run. */}
      {areaCount != null &&
        (areaCount === 0 ? (
          <Item tone="quiet">No areas</Item>
        ) : (
          <Item tone="ok">{plural(areaCount, "area")}</Item>
        ))}

      {missingTestFiles.length > 0 ? (
        <Item tone="warn" title={`Not in the Test files folder: ${missingTestFiles.join(", ")}`}>
          {plural(missingTestFiles.length, "test file")} missing
        </Item>
      ) : testFileCount != null ? (
        testFileCount === 0 ? (
          <Item tone="quiet">No test files</Item>
        ) : (
          <Item tone="ok">{plural(testFileCount, "test file")}</Item>
        )
      ) : null}

      <Button size="sm" variant="ghost" className="ml-auto" onClick={onOpenSetup}>
        Open setup
      </Button>
    </div>
  );
}
