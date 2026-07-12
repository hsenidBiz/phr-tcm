import { useQuery } from "@tanstack/react-query";
import { KanbanSquare, Moon, Settings as SettingsIcon, Sun } from "lucide-react";
import { useState } from "react";
import { commands, type PbiHit } from "../bindings";
import { unwrap } from "../lib/ipc";
import { getTheme, setTheme } from "../lib/theme";
import PbiPicker from "./PbiPicker";
import { Button } from "./ui/button";
import { Select } from "./ui/select";

/** Global scope (org > project > PBI) + mode switch, settings, account,
 * theme. Picked once here; every sidebar screen scopes to it. */
export default function ContextBar({
  org,
  setOrg,
  project,
  setProject,
  pbi,
  setPbi,
  account,
  workMode,
  onToggleWork,
  onOpenSettings,
}: {
  org: string;
  setOrg: (v: string) => void;
  project: string;
  setProject: (v: string) => void;
  pbi: PbiHit | null;
  setPbi: (p: PbiHit | null) => void;
  account: string | null;
  workMode: boolean;
  onToggleWork: () => void;
  onOpenSettings: () => void;
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
    <div className="flex items-center gap-2 border-b border-border bg-surface px-4 py-2.5">
      <Select
        aria-label="Organization"
        className="w-44 py-1.5"
        value={org}
        onChange={(e) => {
          setOrg(e.target.value);
          setProject("");
          setPbi(null);
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
        className="w-44 py-1.5"
        value={project}
        disabled={!org}
        onChange={(e) => {
          setProject(e.target.value);
          setPbi(null);
        }}
      >
        <option value="">Project...</option>
        {(projects.data ?? []).map((p) => (
          <option key={p.id} value={p.name}>
            {p.name}
          </option>
        ))}
      </Select>
      <PbiPicker org={org} project={project} pbi={pbi} onChange={setPbi} />
      {orgs.isError && <span className="text-xs text-danger">{orgs.error.message}</span>}

      <div className="ml-auto flex items-center gap-2">
        <Button variant="pill" size="sm" onClick={onToggleWork}>
          <KanbanSquare size={14} />
          {workMode ? "Test Case Manager" : "Work Manager (Beta)"}
        </Button>
        {account && <span className="text-sm text-muted">{account}</span>}
        <button
          aria-label="Settings"
          className="rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-text"
          onClick={onOpenSettings}
        >
          <SettingsIcon size={16} />
        </button>
        <button
          aria-label="Toggle theme"
          className="rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-text"
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
