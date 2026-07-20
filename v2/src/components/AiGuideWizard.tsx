// 4-step wizard that generates the repo-droppable AI test-case guide.
// Discovery values arrive pre-fetched via React Query; the Rust side is
// pure generation + file writes (no network).
import { useQuery } from "@tanstack/react-query";
import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type GuideFlavor } from "../bindings";
import { loadFieldPrefs } from "../lib/fieldPrefs";
import { unwrap, unwrapStr } from "../lib/ipc";
import { Button } from "./ui/button";
import { Checkbox } from "./ui/checkbox";
import { Textarea } from "./ui/input";
import { Select } from "./ui/select";
import TagField, { splitTags } from "./ui/tagfield";

const FLAVORS: { id: GuideFlavor; label: string }[] = [
  { id: "Generic", label: "Generic markdown (AI_TEST_CASES.md)" },
  { id: "ClaudeSkill", label: "Claude Code skill" },
  { id: "CursorRules", label: "Cursor rules" },
  { id: "AgentsSnippet", label: "AGENTS.md snippet" },
];

const STEP_TITLES = ["Context", "Repo knowledge", "Flavors", "Output"];

/** Toggle set membership, returning a new Set (immutable update for React state). */
function toggled<T>(set: Set<T>, value: T, include: boolean): Set<T> {
  const next = new Set(set);
  if (include) next.add(value);
  else next.delete(value);
  return next;
}

export default function AiGuideWizard({
  org,
  project,
  area,
  onClose,
}: {
  org: string;
  project: string;
  area: string | null;
  onClose: () => void;
}) {
  const [step, setStep] = useState(0);
  const moduleRef = loadFieldPrefs(org, project)?.moduleRef ?? null;

  const modules = useQuery({
    queryKey: ["ai-guide-modules", org, project, moduleRef],
    queryFn: () => unwrap(commands.testCaseFieldValues(org, project, moduleRef!)),
    enabled: Boolean(moduleRef),
    retry: false,
  });
  // Repo + folder browsing for the docs-folder picker (step 2). Repos share
  // the PR panel's cache key; folders re-fetch per drill-down path.
  const [repoId, setRepoId] = useState("");
  const [folderPath, setFolderPath] = useState("/");
  const repos = useQuery({
    queryKey: ["repos", org, project],
    queryFn: () => unwrap(commands.listRepos(org, project)),
    staleTime: 60 * 60_000,
    retry: false,
  });
  const folders = useQuery({
    queryKey: ["ai-guide-folders", org, project, repoId, folderPath],
    queryFn: () => unwrap(commands.listRepoFolders(org, project, repoId, folderPath)),
    enabled: Boolean(repoId),
    retry: false,
  });

  // Modules are explicitly ADDED (search-and-add, like tags elsewhere) -
  // a 100+-value org made a default-all-checked prune list unusable.
  // Semicolon-joined string because that's TagField's value contract.
  const [moduleSel, setModuleSel] = useState("");
  const [pickedDocs, setPickedDocs] = useState<string[]>([]);
  const [docPathsText, setDocPathsText] = useState("");
  const [conventions, setConventions] = useState("");
  const [flavors, setFlavors] = useState<Set<GuideFlavor>>(new Set(["Generic"]));
  const [written, setWritten] = useState<string[] | null>(null);
  const [saving, setSaving] = useState(false);

  const modulesUnavailable = !moduleRef || modules.isError;
  // Honest flag: only true once discovery actually succeeded - an empty
  // result still counts as "discovered", a failed lookup never does.
  const modulesDiscovered = Boolean(moduleRef) && modules.isSuccess;

  const buildOptions = () => ({
    organization: org,
    project,
    area,
    modules: splitTags(moduleSel),
    modules_discovered: modulesDiscovered,
    doc_paths: [
      ...pickedDocs,
      ...docPathsText
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
    ],
    conventions,
    flavors: [...flavors],
    generated_on: new Date().toISOString().slice(0, 10),
  });

  const saveToFolder = async () => {
    if (flavors.size === 0) {
      toast.error("Pick at least one flavor first.");
      return;
    }
    const dir = await open({ directory: true });
    if (typeof dir !== "string") return;
    setSaving(true);
    try {
      setWritten(await unwrapStr(commands.writeAiGuide(dir, buildOptions())));
      toast.success("Guide written.");
    } catch (e) {
      toast.error(`Could not write the guide: ${(e as Error).message}`);
    } finally {
      setSaving(false);
    }
  };

  const copyToClipboard = async () => {
    try {
      const body = await commands.previewAiGuide(buildOptions());
      await navigator.clipboard.writeText(body);
      toast.success("Copied the guide to the clipboard.");
    } catch (e) {
      toast.error(`Could not generate the guide: ${(e as Error).message}`);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4" onClick={onClose}>
      <div
        className="flex max-h-[85vh] w-full max-w-md flex-col gap-4 rounded-lg border border-border bg-surface p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="shrink-0">
          <h2 className="text-sm font-semibold text-text">AI test-case guide</h2>
          <p className="text-xs text-muted">
            Step {step + 1} of {STEP_TITLES.length} - {STEP_TITLES[step]}
          </p>
        </div>

        {/* Only the step body scrolls - the header and the Back/Next footer
            stay pinned, so the dialog never grows past the viewport no
            matter how long a picklist is. */}
        <div className="min-h-0 flex-1 overflow-y-auto pr-1">
        {step === 0 && (
          <div className="space-y-4">
            <p className="text-xs text-muted">
              Generating for <span className="text-text">{org}/{project}</span>
              {area && (
                <>
                  {" "}
                  · area <span className="text-text">{area}</span>
                </>
              )}
            </p>

            <div className="space-y-1.5">
              <p className="text-xs font-semibold text-muted">Modules</p>
              {modulesUnavailable ? (
                <p className="text-xs text-faint">
                  Modules could not be discovered for this project - the guide will skip module
                  guidance.
                </p>
              ) : modules.isLoading ? (
                <p className="text-xs text-faint">Loading modules…</p>
              ) : (
                <>
                  <TagField
                    ariaLabel="Modules"
                    placeholder="Search and add modules…"
                    value={moduleSel}
                    onChange={setModuleSel}
                    suggestions={modules.data ?? []}
                  />
                  <p className="text-xs text-faint">
                    Add the Module values this repo's test cases use - the guide tells AI tools
                    to pick from exactly these.
                  </p>
                </>
              )}
            </div>

          </div>
        )}

        {step === 1 && (
          <div className="space-y-3">
            <div className="space-y-1.5">
              <p className="text-xs font-semibold text-muted">Documentation folder</p>
              <p className="text-xs text-faint">
                Pick the repo you are working in, then browse to its docs/prototype folder.
              </p>
              <Select
                aria-label="Repository"
                className="w-full py-1.5 text-xs"
                value={repoId}
                onChange={(e) => {
                  setRepoId(e.target.value);
                  setFolderPath("/");
                }}
              >
                <option value="">Pick a repository…</option>
                {(repos.data ?? []).map((r) => (
                  <option key={r.id} value={r.id}>
                    {r.name}
                  </option>
                ))}
              </Select>
              {repoId &&
                (folders.isError ? (
                  <p className="text-xs text-faint">
                    Could not browse this repository - add the folder manually below.
                  </p>
                ) : (
                  <div className="space-y-1.5 rounded-md border border-border p-2">
                    {/* The header row stays put while a drill-down loads, so
                        "Use this folder" never jumps out from under the mouse. */}
                    <div className="flex items-center gap-2">
                      <span className="id-mono flex-1 truncate text-xs text-muted">{folderPath}</span>
                      {folderPath !== "/" && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() =>
                            setFolderPath(folderPath.slice(0, folderPath.lastIndexOf("/")) || "/")
                          }
                        >
                          Up
                        </Button>
                      )}
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={folderPath === "/"}
                        onClick={() => {
                          const glob = `${folderPath.replace(/^\//, "")}/**`;
                          setPickedDocs((p) => (p.includes(glob) ? p : [...p, glob]));
                        }}
                      >
                        Use this folder
                      </Button>
                    </div>
                    {folders.isLoading ? (
                      <p className="text-xs text-faint">Loading folders…</p>
                    ) : (folders.data ?? []).length === 0 ? (
                      <p className="text-xs text-faint">No subfolders here.</p>
                    ) : (
                      <div className="max-h-28 space-y-0.5 overflow-y-auto">
                        {folders.data!.map((f) => (
                          <button
                            key={f}
                            className="block w-full truncate rounded px-1.5 py-0.5 text-left text-xs text-text hover:bg-surface-2"
                            onClick={() => setFolderPath(f)}
                          >
                            {f.slice(f.lastIndexOf("/") + 1)}/
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                ))}
              {pickedDocs.length > 0 && (
                <ul className="space-y-0.5">
                  {pickedDocs.map((p) => (
                    <li key={p} className="flex items-center gap-2 text-xs text-text">
                      <span className="id-mono flex-1 truncate">{p}</span>
                      <button
                        aria-label={`Remove ${p}`}
                        className="text-faint hover:text-danger"
                        onClick={() => setPickedDocs((d) => d.filter((x) => x !== p))}
                      >
                        ✕
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
            <label className="block text-xs text-muted">
              Documentation paths (manual)
              <Textarea
                aria-label="Documentation paths"
                className="mt-1 h-14 w-full font-mono text-xs"
                placeholder={"docs/screens/**\nREADME.md"}
                value={docPathsText}
                onChange={(e) => setDocPathsText(e.target.value)}
              />
              <span className="mt-1 block text-faint">
                One glob or path per line, relative to the repo root - use this when the folder
                isn't in the repo yet.
              </span>
            </label>
            <label className="block text-xs text-muted">
              Team conventions
              <Textarea
                aria-label="Team conventions"
                className="mt-1 h-14 w-full"
                placeholder="Anything AI tools should know about how your team writes test cases…"
                value={conventions}
                onChange={(e) => setConventions(e.target.value)}
              />
            </label>
          </div>
        )}

        {step === 2 && (
          <div className="space-y-1.5">
            <p className="text-xs text-muted">Pick one or more output flavors for the guide.</p>
            {FLAVORS.map((f) => (
              <label key={f.id} className="flex items-center gap-2 text-xs text-text">
                <Checkbox
                  ariaLabel={f.label}
                  checked={flavors.has(f.id)}
                  onCheckedChange={(checked) => setFlavors((p) => toggled(p, f.id, checked))}
                />
                {f.label}
              </label>
            ))}
          </div>
        )}

        {step === 3 &&
          (written ? (
            <div className="space-y-2">
              <p className="text-xs text-muted">Wrote:</p>
              <ul className="space-y-1">
                {written.map((f) => (
                  <li key={f} className="text-xs text-text">
                    {f}
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <div className="space-y-3">
              <p className="text-xs text-muted">
                Save the guide into your repo, or copy its contents to paste elsewhere.
              </p>
              <div className="flex gap-2">
                <Button variant="outline" size="sm" disabled={saving} onClick={saveToFolder}>
                  {saving ? "Saving…" : "Save to folder…"}
                </Button>
                <Button variant="outline" size="sm" onClick={copyToClipboard}>
                  Copy to clipboard
                </Button>
              </div>
            </div>
          ))}
        </div>

        <div className="flex shrink-0 justify-end gap-2 pt-1">
          {written ? (
            <Button size="sm" onClick={onClose}>
              Done
            </Button>
          ) : (
            <>
              <Button variant="ghost" size="sm" onClick={onClose}>
                Cancel
              </Button>
              {step > 0 && (
                <Button variant="outline" size="sm" onClick={() => setStep((s) => s - 1)}>
                  Back
                </Button>
              )}
              {step < STEP_TITLES.length - 1 && (
                <Button size="sm" onClick={() => setStep((s) => s + 1)}>
                  Next
                </Button>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
