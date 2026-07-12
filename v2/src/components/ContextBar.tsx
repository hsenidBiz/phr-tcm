import { useQuery } from "@tanstack/react-query";
import { Moon, Sun } from "lucide-react";
import { useState } from "react";
import { commands } from "../bindings";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
import { getTheme, setTheme } from "../lib/theme";
import { Select } from "./ui/select";

/** Org/project scope + account + theme toggle. Picked once, everything
 * (Test Cases hub, Work board) scopes to it. */
export default function ContextBar({
  org,
  setOrg,
  project,
  setProject,
  account,
}: {
  org: string;
  setOrg: (v: string) => void;
  project: string;
  setProject: (v: string) => void;
  account: string | null;
}) {
  const [dark, setDark] = useState(
    () => document.documentElement.classList.contains("dark") || getTheme() !== "light",
  );

  const orgs = useQuery({
    queryKey: ["orgs"],
    queryFn: () => unwrap(commands.listOrgs()),
  });

  const projects = useQuery({
    queryKey: ["projects", org],
    queryFn: () => unwrap(commands.listProjects(org)),
    enabled: Boolean(org),
  });

  return (
    <div className="flex items-center gap-3 border-b border-border bg-surface px-4 py-2.5">
      <Select
        aria-label="Organization"
        className="w-52 py-1.5"
        value={org}
        onChange={(e) => {
          setOrg(e.target.value);
          setProject("");
        }}
      >
        <option value="">Organization...</option>
        {(orgs.data ?? []).map((o) => (
          <option key={o.name} value={o.name}>
            {o.name}
          </option>
        ))}
      </Select>
      <Select
        aria-label="Project"
        className="w-52 py-1.5"
        value={project}
        disabled={!org}
        onChange={(e) => setProject(e.target.value)}
      >
        <option value="">Project...</option>
        {(projects.data ?? []).map((p) => (
          <option key={p.id} value={p.name}>
            {p.name}
          </option>
        ))}
      </Select>
      {orgs.isError && <span className="text-xs text-danger">{orgs.error.message}</span>}

      <div className="ml-auto flex items-center gap-3">
        {account && <span className="text-sm text-muted">{account}</span>}
        <button
          aria-label="Toggle theme"
          className={cn(
            "rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-text",
          )}
          onClick={() => {
            const next = !dark;
            setDark(next);
            setTheme(next ? "dark" : "light");
          }}
        >
          {dark ? <Sun size={16} /> : <Moon size={16} />}
        </button>
      </div>
    </div>
  );
}
