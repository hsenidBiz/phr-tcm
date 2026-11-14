import { useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { commands } from "../../bindings";
import { cn } from "../../lib/cn";
import { fileName } from "../../lib/fileSync";
import { Button } from "../ui/button";

/** One recently imported file. The row asks the disk whether the file
 * still exists (fileStamp is null for a missing one) and says so instead
 * of offering a click that can only fail - deleted files happen, and the
 * row's job is to be honest about them. The X forgets the entry either
 * way. */
export default function RecentImportRow({
  path,
  when,
  onOpen,
  onForget,
}: {
  path: string;
  when: number;
  onOpen: () => void;
  onForget: () => void;
}) {
  const exists = useQuery({
    queryKey: ["file-exists", path],
    queryFn: () => commands.fileStamp(path),
    // A file can come back (restored from the bin, a re-synced share) -
    // re-ask on mount rather than trusting a stale "gone".
    staleTime: 0,
    retry: false,
  });
  const missing = exists.isSuccess && exists.data === null;
  return (
    <div
      className={cn(
        "flex items-center gap-2 rounded-md border border-border bg-surface px-3 py-1.5 text-xs",
        missing && "opacity-70",
      )}
    >
      <span className="min-w-0 flex-1 truncate">
        <span className="text-text">{fileName(path)}</span>
        <span className="ml-2 text-faint" title={path}>
          {path}
        </span>
      </span>
      <span className="shrink-0 text-faint">{new Date(when).toLocaleDateString()}</span>
      {missing ? (
        <span className="shrink-0 rounded-full bg-warning/15 px-2 py-0.5 text-[10px] font-medium text-warning">
          <span className="label-trim">File no longer exists</span>
        </span>
      ) : (
        <Button
          size="sm"
          variant="outline"
          aria-label={`Reopen ${fileName(path)}`}
          disabled={exists.isLoading}
          onClick={onOpen}
        >
          Open
        </Button>
      )}
      <button
        aria-label={`Remove ${fileName(path)} from recent imports`}
        title="Remove from recent imports"
        className="shrink-0 rounded p-1 text-muted transition-colors hover:text-danger"
        onClick={onForget}
      >
        <X size={13} />
      </button>
    </div>
  );
}
