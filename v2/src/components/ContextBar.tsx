import { useQuery } from "@tanstack/react-query";
import { KanbanSquare, Settings as SettingsIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { commands, type PbiHit } from "../bindings";
import { cn } from "../lib/cn";
import { unwrap } from "../lib/ipc";
import { PBI_GLOW_EVENT } from "../lib/pbiGlow";
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
  settingsOpen = false,
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
  settingsOpen?: boolean;
}) {
  // The review gate's final confirmation spotlights the PBI chip so the
  // user verifies the target before an irreversible create.
  const [pbiGlow, setPbiGlow] = useState(false);
  useEffect(() => {
    const onGlow = (e: Event) => setPbiGlow(Boolean((e as CustomEvent).detail));
    window.addEventListener(PBI_GLOW_EVENT, onGlow);
    return () => window.removeEventListener(PBI_GLOW_EVENT, onGlow);
  }, []);

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
    // flex-wrap: in a narrow window the right-side group drops to a second
    // row instead of overlapping the PBI picker.
    <div className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-border bg-surface px-4 py-2.5">
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
      {/* The PBI chip gets all remaining width so long titles stay readable;
          min-w keeps it usable and forces a wrap instead of a squeeze. */}
      <div className={cn("min-w-56 flex-1", pbiGlow && "pbi-glow")} data-tour="pbi">
        <PbiPicker org={org} project={project} pbi={pbi} onChange={setPbi} />
        {/* Comet trail: dots staggered across half a 7s lap = a tail
            covering half the perimeter, fading toward its end. */}
        {pbiGlow &&
          Array.from({ length: 22 }, (_, i) => {
            const t = i / 21; // 0 = head, 1 = tail end
            return (
              <span
                key={i}
                aria-hidden
                className="pbi-comet"
                style={{
                  width: 7 - t * 4.5,
                  height: 7 - t * 4.5,
                  opacity: 1 - t * 0.95,
                  animationDelay: `${-t * 3.5}s`,
                }}
              />
            );
          })}
      </div>
      {orgs.isError && <span className="text-xs text-danger">{orgs.error.message}</span>}

      <div className="ml-auto flex shrink-0 items-center gap-2">
        <Button
          data-tour="work"
          variant="pill"
          size="sm"
          title={workMode ? "Test Case Manager" : "Work Manager (Beta)"}
          onClick={onToggleWork}
        >
          <KanbanSquare size={14} />
          {/* Icon-only below lg so the button never crowds the PBI picker. */}
          <span className="hidden lg:inline">
            {workMode ? "Test Case Manager" : "Work Manager (Beta)"}
          </span>
        </Button>
        {account && <span className="hidden text-sm text-muted xl:inline">{account}</span>}
        <button
          data-tour="settings"
          aria-label={settingsOpen ? "Close settings" : "Settings"}
          title={settingsOpen ? "Close settings" : "Settings"}
          aria-pressed={settingsOpen}
          className={
            settingsOpen
              ? "rounded-md bg-accent-soft p-2 text-accent transition-colors"
              : "rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-text"
          }
          onClick={onOpenSettings}
        >
          <SettingsIcon size={16} />
        </button>
      </div>
    </div>
  );
}
