import type { ChangelogEntry } from "../lib/changelog";
import { isBetaVersion } from "../lib/changelogSeen";
import BetaPill from "./BetaPill";

/** "Version 1.26.0-beta.1 [Beta] 2026-10-01" - the heading of one version's
 *  notes, shared by What's new and Settings' changelog. */
export default function ChangelogVersionTitle({ entry }: { entry: ChangelogEntry }) {
  return (
    <h3 className="text-xs font-semibold text-text">
      Version {entry.version}
      {isBetaVersion(entry.version) && (
        <>
          {" "}
          <BetaPill className="ml-2" />
        </>
      )}
      <span className="ml-2 font-normal text-faint">{entry.date}</span>
    </h3>
  );
}
