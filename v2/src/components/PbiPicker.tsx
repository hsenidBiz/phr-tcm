import { useQuery } from "@tanstack/react-query";
import { History, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { commands, type PbiHit } from "../bindings";
import { unwrap } from "../lib/ipc";
import { Input } from "./ui/input";

const RECENTS_MAX = 8;
const recentsKey = (org: string, project: string) => `tcm-v2-recent-pbis:${org}/${project}`;

export function loadRecents(org: string, project: string): PbiHit[] {
  try {
    const raw = localStorage.getItem(recentsKey(org, project));
    return raw ? (JSON.parse(raw) as PbiHit[]) : [];
  } catch {
    return [];
  }
}

function pushRecent(org: string, project: string, pbi: PbiHit) {
  try {
    const next = [pbi, ...loadRecents(org, project).filter((p) => p.id !== pbi.id)].slice(
      0,
      RECENTS_MAX,
    );
    localStorage.setItem(recentsKey(org, project), JSON.stringify(next));
  } catch {
    // storage unavailable -> no recents
  }
}

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
  const boxRef = useRef<HTMLDivElement>(null);

  // Click anywhere outside to dismiss. Captured `pointerdown`, not click:
  // it fires before whatever was clicked handles its own press, and it
  // still reaches us if that thing stops propagation. A press INSIDE
  // (a result, the input) is ignored, so picking still works.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!boxRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [open]);

  const recents = loadRecents(org, project);
  const pick = (hit: PbiHit) => {
    pushRecent(org, project, hit);
    onChange(hit);
    setOpen(false);
  };

  const hits = useQuery({
    queryKey: ["pbis", org, project, query],
    queryFn: () => unwrap(commands.searchPbis(org, project, query)),
    enabled: Boolean(org && project && query),
    retry: false,
  });

  if (pbi) {
    return (
      <span className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md border border-accent/50 bg-accent-soft px-2.5 py-1.5 text-sm">
        <span className="id-mono text-faint">#{pbi.id}</span>
        <span className="min-w-0 truncate text-text">{pbi.title}</span>
        <button
          aria-label="Clear PBI"
          className="ml-1 shrink-0 text-muted hover:text-danger"
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
    <div className="relative" ref={boxRef}>
      <Input
        aria-label="Find PBI"
        className="w-72 py-1.5"
        placeholder={project ? "Find PBI (Enter to search)" : "Pick a project first"}
        disabled={!project}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onFocus={() => {
          if (!text && recents.length > 0) setOpen(true);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            setQuery(text.trim());
            setOpen(true);
          }
          if (e.key === "Escape") setOpen(false);
        }}
      />
      {open && (query || recents.length > 0) && (
        <div className="absolute left-0 top-full z-30 mt-1 w-96 rounded-md border border-border bg-surface shadow-xl">
          {query && hits.isLoading && <p className="px-3 py-2 text-sm text-muted">Searching</p>}
          {query && hits.isError && (
            <p className="px-3 py-2 text-sm text-danger">{hits.error.message}</p>
          )}
          {query && hits.data && hits.data.length === 0 && (
            <p className="px-3 py-2 text-sm text-muted">No PBIs match "{query}".</p>
          )}
          <ul className="max-h-72 overflow-y-auto p-1">
            {(query ? hits.data ?? [] : []).map((hit) => (
              <li key={hit.id}>
                <button
                  className="w-full rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft"
                  onClick={() => pick(hit)}
                >
                  <span className="id-mono text-faint">#{hit.id}</span> {hit.title}
                </button>
              </li>
            ))}
            {!query && recents.length > 0 && (
              <>
                <li className="px-2 py-1 text-[10px] uppercase tracking-wide text-faint">
                  Recently used
                </li>
                {recents.map((hit) => (
                  <li key={hit.id}>
                    <button
                      className="flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left text-sm text-text hover:bg-accent-soft"
                      onClick={() => pick(hit)}
                    >
                      <History size={12} className="text-faint" />
                      <span className="id-mono text-faint">#{hit.id}</span> {hit.title}
                    </button>
                  </li>
                ))}
              </>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
