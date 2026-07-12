import { useQuery } from "@tanstack/react-query";
import { KanbanSquare, Settings as SettingsIcon } from "lucide-react";
import { commands, type PbiHit } from "../bindings";
import { unwrap } from "../lib/ipc";
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
        data-tour="org"
        aria-label="Organization"
        className="w-44 py-1.5"
        value={org}
        onChange={(e) => {
          setOrg(e.target.value);
          setProject("");
          setPbi(null);
        }}
      >
        <option value="">Organization</option>
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
        <option value="">Project</option>
        {(projects.data ?? []).map((p) => (
          <option key={p.id} value={p.name}>
            {p.name}
          </option>
        ))}
      </Select>
      {/* The PBI chip gets all remaining width so long titles stay readable. */}
      <div className="min-w-0 flex-1" data-tour="pbi">
        <PbiPicker org={org} project={project} pbi={pbi} onChange={setPbi} />
      </div>
      {orgs.isError && <span className="text-xs text-danger">{orgs.error.message}</span>}

      <div className="flex shrink-0 items-center gap-2">
        <Button data-tour="work" variant="pill" size="sm" onClick={onToggleWork}>
          <KanbanSquare size={14} />
          {workMode ? "Test Case Manager" : "Work Manager (Beta)"}
        </Button>
        {account && <span className="text-sm text-muted">{account}</span>}
        <button
          data-tour="settings"
          aria-label="Settings"
          className="rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-text"
          onClick={onOpenSettings}
        >
          <SettingsIcon size={16} />
        </button>
      </div>
    </div>
  );
}
