import { useQuery } from "@tanstack/react-query";
import { Settings as SettingsIcon } from "lucide-react";
import { IconBoard, IconTestCases } from "../lib/actionIcons";
import { useEffect, useRef, useState } from "react";
import { commands, type PbiHit } from "../bindings";
import ElectricBorder from "./ElectricBorder";
import { useMentions } from "../hooks/useMentions";
import { usePrAttention } from "../hooks/usePrAttention";
import NotificationBell from "./NotificationBell";
import type { NotificationTarget } from "../lib/notifications";
import { unwrap } from "../lib/ipc";
import { cssLengthPx } from "../lib/cssLength";
import { CACHE, cacheKeys, persistentQuery } from "../lib/cache";
import { cn } from "../lib/cn";
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
  allProjects,
  homeProject,
  pbi,
  setPbi,
  account,
  workMode,
  onToggleWork,
  onOpenSettings,
  settingsOpen = false,
  locked = false,
  workLive = false,
  settingsLive = false,
  onOpenNotification,
}: {
  org: string;
  setOrg: (v: string) => void;
  /** The effective project: the PBI's own while the projects show
   * together, otherwise the picked one. */
  project: string;
  setProject: (v: string) => void;
  /** The projects show together: no Project picker, and what is not about
   * one PBI is checked in the home project. */
  allProjects: boolean;
  homeProject: string;
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
  /** Same for the gear: Settings has no rail row, so the tour's stops
   * there ask for this button and nothing else. */
  settingsLive?: boolean;
  /** A bell notification was clicked: take the app to what it is about. */
  onOpenNotification?: (target: NotificationTarget) => void;
}) {
  // The review gate's final confirmation spotlights the PBI chip so the
  // user verifies the target before an irreversible create.
  const [pbiGlow, setPbiGlow] = useState(false);
  // The gear turns forward when Settings opens and back when it closes -
  // however it closes (the gear, the sidebar, the command palette). `n`
  // remounts the icon so each change plays afresh (index.css, ico-cog-turn).
  const [turn, setTurn] = useState<{ n: number; opening: boolean } | null>(null);
  const wasOpen = useRef(settingsOpen);
  useEffect(() => {
    if (wasOpen.current === settingsOpen) return;
    wasOpen.current = settingsOpen;
    setTurn((t) => ({ n: (t?.n ?? 0) + 1, opening: settingsOpen }));
  }, [settingsOpen]);
  useEffect(() => {
    const onGlow = (e: Event) => setPbiGlow(Boolean((e as CustomEvent).detail));
    window.addEventListener(PBI_GLOW_EVENT, onGlow);
    return () => window.removeEventListener(PBI_GLOW_EVENT, onGlow);
  }, []);

  // Org/project lists barely change - served from the local cache for a
  // day, so most app starts cost zero ADO requests here.
  const orgs = useQuery({
    queryKey: ["orgs"],
    ...persistentQuery({
      key: cacheKeys.orgs(),
      fetcher: () => unwrap(commands.listOrgs()),
      ...CACHE.reference,
    }),
  });

  const projects = useQuery({
    queryKey: ["projects", org],
    ...persistentQuery({
      key: cacheKeys.projects(org),
      fetcher: () => unwrap(commands.listProjects(org)),
      ...CACHE.reference,
    }),
    enabled: Boolean(org),
  });

  // An account with exactly one organization has nothing to pick: that
  // organization is chosen for you and the picker is not drawn. Zero or
  // several keep the picker. The tour (`locked`) swaps the list for its
  // sample one and must not be pulled back to the real organization.
  const singleOrg = orgs.data?.length === 1 ? orgs.data[0].name : null;
  useEffect(() => {
    if (locked || !singleOrg) return;
    if (org !== singleOrg) {
      // The same three calls the picker's onChange makes.
      setOrg(singleOrg);
      setProject("");
      setPbi(null);
    }
    // The setters are the parent's and change identity every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [singleOrg, org, locked]);

  // The Project picker is for one-project-at-a-time work only, and a single
  // project is chosen for you by App (the same rule as the organization).
  const showProjectPicker = !allProjects && (projects.data?.length ?? 0) !== 1;

  // PRs with conflicts or comments still to resolve - the number on the
  // Work Manager pill. Shares the PR panel's cache keys. Together it reads
  // every project's pull requests, and the mention check below covers every
  // project too; the home project only fills in for what names none (so
  // nothing here jumps with every PBI picked).
  const watchedProject = allProjects ? homeProject : project;
  const prAttention = usePrAttention(org, watchedProject, allProjects);
  // Work-item @mentions of you, into the bell beside it.
  useMentions(org, watchedProject, allProjects);

  return (
    // flex-wrap: in a narrow window the right-side group drops to a second
    // row instead of overlapping the PBI picker.
    <div className="flex flex-wrap items-center gap-x-2 gap-y-2 border-b border-border bg-surface px-4 py-2.5">
      {/* The scope group: Organization (not drawn when there is exactly one),
          Project (not drawn while the projects show together, or when there
          is exactly one) and the PBI search. Always drawn, because the tour anchors
          its "where you work" stop here. It takes all the free width (flex-1)
          and never less than 224px (min-w-56), so the right-hand group keeps
          the first row. With several organizations on a narrow header the
          three controls do not fit side by side: the PBI search wraps under
          the pickers inside this group while the right-hand group stays on
          the first row. */}
      <div
        data-tour="org"
        className="flex min-w-56 flex-1 flex-wrap items-center gap-x-2 gap-y-2"
      >
        {singleOrg === null && (
          <Select
            aria-label="Organization"
            className="w-44" triggerClassName="h-10 py-0"
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
        )}
        {showProjectPicker && (
          <Select
            aria-label="Project"
            className="w-44" triggerClassName="h-10 py-0"
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
        )}
        {/* The PBI search takes ALL the width between the Project picker and
            the right-hand group, with no maximum: the group never shrinks, so
            this is what gives way. min-w forces a wrap instead of a squeeze. */}
        <div className="min-w-56 flex-1" data-tour="pbi" inert={locked}>
          {/* React Bits ElectricBorder wraps the chip while confirmation is
              armed, in the theme accent so it follows light/dark and presets. */}
          {pbiGlow ? (
            <ElectricBorder
              color={getComputedStyle(document.documentElement)
                .getPropertyValue("--color-accent")
                .trim()}
              speed={1}
              chaos={0.01}
              // The chip's own corners (rounded-md), read from the same token,
              // so the glow traces the control instead of a rounder box.
              borderRadius={cssLengthPx("--radius-md", 6)}
            >
              <PbiPicker org={org} project={project} pbi={pbi} onChange={setPbi} perProject={!allProjects} />
            </ElectricBorder>
          ) : (
            <PbiPicker org={org} project={project} pbi={pbi} onChange={setPbi} perProject={!allProjects} />
          )}
        </div>
      </div>
      {orgs.isError && <span className="text-xs text-danger">{orgs.error.message}</span>}

      <div className="ml-auto flex shrink-0 items-center gap-2">
        <NotificationBell org={org} onOpen={onOpenNotification} />
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
          {/* No count on the pill: the bell beside it is the one place
              numbers live, and it carries this one too ("comments to
              resolve"). The tooltip still says why Work Manager is worth
              opening. */}
        </Button>
        {account && <span className="hidden text-sm text-muted xl:inline">{account}</span>}
        <button
          data-tour="settings"
          aria-label={settingsOpen ? "Close settings" : "Settings"}
          title={settingsOpen ? "Close settings" : "Settings"}
          aria-pressed={settingsOpen}
          // A fixed square that centres its icon, so the gear never rides a
          // text baseline; the ring is the one the app's Buttons draw.
          className={cn(
            "inline-flex size-8 items-center justify-center rounded-md transition-colors focus-visible:outline-2 focus-visible:outline-accent disabled:pointer-events-none",
            settingsOpen
              ? "bg-accent-soft text-accent"
              : "text-muted hover:bg-surface-2 hover:text-text",
          )}
          // The tour spotlights the gear and, on its Settings stops, asks
          // the user to click it - it never opens Settings for them.
          disabled={locked && !settingsLive}
          onClick={() => {
            if (locked && !settingsLive) return;
            onOpenSettings();
          }}
        >
          <SettingsIcon
            key={turn?.n ?? 0}
            size={16}
            className={cn(
              "block",
              turn && (turn.opening ? "ico-cog-turn" : "ico-cog-turn-back"),
            )}
          />
        </button>
      </div>
    </div>
  );
}
