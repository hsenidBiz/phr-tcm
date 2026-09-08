import { useQuery } from "@tanstack/react-query";
import { Settings as SettingsIcon } from "lucide-react";
import { IconBoard, IconTestCases } from "../lib/actionIcons";
import { useEffect, useState } from "react";
import { commands, type PbiHit } from "../bindings";
import ElectricBorder from "./ElectricBorder";
import { usePrAttention } from "../hooks/usePrAttention";
import { unwrap } from "../lib/ipc";
import { cached } from "../lib/localCache";
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
  locked = false,
  workLive = false,
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
  /** The guided tour is running: this bar sits outside the inert shell so
   * its Work Manager pill can be the one live control, which means every
   * other control here has to lock itself. */
  locked?: boolean;
  /** ...and the pill is live only when the current stop is waiting for
   * the user to cross between the two halves of the app. */
  workLive?: boolean;
}) {
  // The review gate's final confirmation spotlights the PBI chip so the
  // user verifies the target before an irreversible create.
  const [pbiGlow, setPbiGlow] = useState(false);
  useEffect(() => {
    const onGlow = (e: Event) => setPbiGlow(Boolean((e as CustomEvent).detail));
    window.addEventListener(PBI_GLOW_EVENT, onGlow);
    return () => window.removeEventListener(PBI_GLOW_EVENT, onGlow);
  }, []);

  // Org/project lists barely change - served from the local cache for a
  // day, so most app starts cost zero ADO requests here.
  const orgs = useQuery({
    queryKey: ["orgs"],
    queryFn: () => cached("orgs", 24 * 60 * 60_000, () => unwrap(commands.listOrgs())),
    staleTime: 60 * 60_000,
  });

  const projects = useQuery({
    queryKey: ["projects", org],
    queryFn: () =>
      cached(`projects:${org}`, 24 * 60 * 60_000, () => unwrap(commands.listProjects(org))),
    enabled: Boolean(org),
    staleTime: 60 * 60_000,
  });

  // PRs with conflicts or comments still to resolve - the number on the
  // Work Manager pill. Shares the PR panel's cache keys.
  const prAttention = usePrAttention(org, project);

  return (
    // flex-wrap: in a narrow window the right-side group drops to a second
    // row instead of overlapping the PBI picker.
    <div className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-border bg-surface px-4 py-2.5">
      <Select
        data-tour="org"
        aria-label="Organization"
        className="w-44" triggerClassName="py-1.5"
        disabled={locked}
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
        className="w-44" triggerClassName="py-1.5"
        value={project}
        disabled={!org || locked}
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
      <div className="min-w-56 flex-1" data-tour="pbi" inert={locked}>
        {/* React Bits ElectricBorder wraps the chip while confirmation is
            armed, in the theme accent so it follows light/dark and presets. */}
        {pbiGlow ? (
          <ElectricBorder
            color={getComputedStyle(document.documentElement)
              .getPropertyValue("--color-accent")
              .trim()}
            speed={1}
            chaos={0.07}
            borderRadius={16}
          >
            <PbiPicker org={org} project={project} pbi={pbi} onChange={setPbi} />
          </ElectricBorder>
        ) : (
          <PbiPicker org={org} project={project} pbi={pbi} onChange={setPbi} />
        )}
      </div>
      {orgs.isError && <span className="text-xs text-danger">{orgs.error.message}</span>}

      <div className="ml-auto flex shrink-0 items-center gap-2">
        <Button
          data-tour="work"
          variant="pill"
          size="sm"
          className="relative"
          title={
            !workMode && prAttention > 0
              ? `${prAttention} pull request${prAttention === 1 ? "" : "s"} with conflicts or comments to resolve`
              : workMode
                ? "Test Case Manager"
                : "Work Manager"
          }
          disabled={locked && !workLive}
          onClick={() => {
            if (locked && !workLive) return;
            onToggleWork();
          }}
        >
          {/* The icon names the DESTINATION, same as the label: a board
              on the way out to Work Manager, the flask on the way back.
              A board glyph in both directions read as "you are here". */}
          {workMode ? <IconTestCases aria-hidden /> : <IconBoard aria-hidden />}
          {/* Icon-only below lg so the button never crowds the PBI picker. */}
          <span className="hidden lg:inline">
            {workMode ? "Test Case Manager" : "Work Manager"}
          </span>
          {/* PRs that need a human: conflicts or comments to resolve.
              Only while the pill points TO Work Manager - the count is
              the reason to go there. Once switched, the pill reads "Test
              Case Manager", and a badge riding on it looked like TCM had
              notifications; inside Work Manager the PR panel itself shows
              what needs attention. */}
          {!workMode && prAttention > 0 && (
            <span
              aria-label={`${prAttention} pull requests need attention`}
              className="absolute -right-1.5 -top-1.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-danger px-1 text-[10px] font-semibold leading-none text-on-accent"
            >
              {prAttention > 99 ? "99+" : prAttention}
            </span>
          )}
        </Button>
        {account && <span className="hidden text-sm text-muted xl:inline">{account}</span>}
        <button
          data-tour="settings"
          aria-label={settingsOpen ? "Close settings" : "Settings"}
          title={settingsOpen ? "Close settings" : "Settings"}
          aria-pressed={settingsOpen}
          className={
            settingsOpen
              ? "rounded-md bg-accent-soft p-2 text-accent transition-colors disabled:pointer-events-none"
              : "rounded-md p-2 text-muted transition-colors hover:bg-surface-2 hover:text-text disabled:pointer-events-none"
          }
          // The tour spotlights the gear; it never opens it.
          disabled={locked}
          onClick={() => !locked && onOpenSettings()}
        >
          <SettingsIcon size={16} />
        </button>
      </div>
    </div>
  );
}
