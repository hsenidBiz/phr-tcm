// The files a case's steps saved during an unattended run, under its steps
// in Past runs and the review: each by name, with its size read from the
// run's download folder now, and an Open button that hands it to the
// system's default app. Rust only opens a plain name in that run's own
// folder; a file that has gone from the folder says so and cannot be opened.

import { useMutation, useQuery } from "@tanstack/react-query";
import { commands } from "../../bindings";
import { Button } from "../../components/ui/button";
import { cn } from "../../lib/cn";
import { unwrapStr } from "../../lib/ipc";
import { toast } from "../../lib/toast";
import { fileSize } from "./TestFilesDialog";

/** What a row says in place of a size when the file is gone. */
export const DOWNLOAD_GONE = "no longer on this machine";

/** The query every list of one run's downloads shares. */
export const downloadSizesKey = (runId: string) => ["autorun-download-sizes", runId] as const;

export default function RunDownloads({
  runId,
  steps,
  className,
}: {
  runId: string;
  /** The case's steps; only their `downloads` are read. Absent reads as
   * none, as a run file written before steps were recorded has. */
  steps?: ReadonlyArray<{ downloads?: string[] }>;
  className?: string;
}) {
  const names = [...new Set((steps ?? []).flatMap((s) => s.downloads ?? []))];
  const sizes = useQuery({
    queryKey: downloadSizesKey(runId),
    queryFn: () => unwrapStr(commands.autoRunDownloadSizes(runId)),
    enabled: names.length > 0,
    // Read again every time a list mounts, whatever the app's defaults: a
    // file removed since the last look must show as gone.
    staleTime: 0,
    refetchOnMount: "always",
    retry: false,
  });
  const open = useMutation({
    mutationFn: (name: string) => unwrapStr(commands.autoRunOpenDownload(runId, name)),
    onError: (e, name) => toast.error(`Could not open ${name}: ${e.message}`),
  });

  if (names.length === 0) return null;
  /** Unknown until the folder has been read: no size shown, Open allowed. */
  const known = sizes.data ? new Map(sizes.data.map((f) => [f.name, f.size])) : null;

  return (
    <div className={cn("space-y-1", className)}>
      <p className="text-xs font-medium text-muted">Downloads</p>
      <ul aria-label="Downloads" className="space-y-1">
        {names.map((name) => {
          const size = known?.get(name);
          const gone = known !== null && size === undefined;
          return (
            <li key={name} className="flex items-center gap-2 text-xs">
              <span className="min-w-0 flex-1 truncate text-text" title={name}>
                {name}
              </span>
              {known !== null && (
                <span className="shrink-0 text-faint">{gone ? DOWNLOAD_GONE : fileSize(size ?? 0)}</span>
              )}
              <Button
                size="sm"
                variant="outline"
                aria-label={`Open ${name}`}
                title={gone ? `${name} is ${DOWNLOAD_GONE}` : "Open this file in its default app"}
                disabled={gone || open.isPending}
                onClick={() => open.mutate(name)}
              >
                Open
              </Button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
