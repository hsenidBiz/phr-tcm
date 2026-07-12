import { useQuery } from "@tanstack/react-query";
import { X } from "lucide-react";
import { useState } from "react";
import { commands, type PbiHit } from "../bindings";
import { unwrap } from "../lib/ipc";
import { Input } from "./ui/input";

/** Global PBI scope in the context bar - v1's Config-screen PBI search.
 * Type, Enter to search, click a hit; everything scopes to the choice. */
export default function PbiPicker({
  org,
  project,
  pbi,
  onChange,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  onChange: (pbi: PbiHit | null) => void;
}) {
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);

  const hits = useQuery({
    queryKey: ["pbis", org, project, query],
    queryFn: () => unwrap(commands.searchPbis(org, project, query)),
    enabled: Boolean(org && project && query),
    retry: false,
  });

  if (pbi) {
    return (
      <span className="flex max-w-md items-center gap-1.5 rounded-md border border-accent/50 bg-accent-soft px-2.5 py-1.5 text-sm">
        <span className="text-faint">#{pbi.id}</span>
        <span className="truncate text-text">{pbi.title}</span>
        <button
          aria-label="Clear PBI"
          className="ml-1 text-muted hover:text-danger"
          onClick={() => {
            onChange(null);
            setText("");
            setQuery("");
          }}
        >
          <X size={13} />
        </button>
      </span>
    );
  }

  return (
    <div className="relative">
      <Input
        aria-label="Find PBI"
        className="w-72 py-1.5"
        placeholder={project ? "Find PBI (Enter to search)" : "Pick a project first"}
        disabled={!project}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            setQuery(text.trim());
            setOpen(true);
          }
          if (e.key === "Escape") setOpen(false);
        }}
      />
      {open && query && (
        <div className="absolute left-0 top-full z-30 mt-1 w-96 rounded-md border border-border bg-surface shadow-xl">
          {hits.isLoading && <p className="px-3 py-2 text-sm text-muted">Searching...</p>}
          {hits.isError && <p className="px-3 py-2 text-sm text-danger">{hits.error.message}</p>}
          {hits.data && hits.data.length === 0 && (
            <p className="px-3 py-2 text-sm text-muted">No PBIs match "{query}".</p>
          )}
          <ul className="max-h-72 overflow-y-auto p-1">
            {(hits.data ?? []).map((hit) => (
              <li key={hit.id}>
                <button
                  className="w-full rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft"
                  onClick={() => {
                    onChange(hit);
                    setOpen(false);
                  }}
                >
                  <span className="text-faint">#{hit.id}</span> {hit.title}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
