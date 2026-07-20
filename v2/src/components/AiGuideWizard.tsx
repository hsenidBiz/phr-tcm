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
  const tags = useQuery({
    queryKey: ["ai-guide-tags", org, project],
    queryFn: () => unwrap(commands.listProjectTags(org, project)),
    retry: false,
  });

  // Pruning state: every discovered value starts checked (included); an
  // unticked box excludes that value from the generated options.
  const [pruned, setPruned] = useState<Set<string>>(new Set());
  const [prunedTags, setPrunedTags] = useState<Set<string>>(new Set());
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
    modules: (modules.data ?? []).filter((m) => !pruned.has(m)),
    tags: (tags.data ?? []).filter((t) => !prunedTags.has(t)),
    modules_discovered: modulesDiscovered,
    doc_paths: docPathsText
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean),
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
        className="w-full max-w-md space-y-4 rounded-lg border border-border bg-surface p-5 shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div>
          <h2 className="text-sm font-semibold text-text">AI test-case guide</h2>
          <p className="text-xs text-muted">
            Step {step + 1} of {STEP_TITLES.length} - {STEP_TITLES[step]}
          </p>
        </div>

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
              ) : (modules.data ?? []).length === 0 ? (
                <p className="text-xs text-faint">No modules in use yet.</p>
              ) : (
                <div className="flex flex-wrap gap-x-4 gap-y-1.5">
                  {modules.data!.map((m) => (
                    <label key={m} className="flex items-center gap-1.5 text-xs text-text">
                      <Checkbox
                        ariaLabel={m}
                        checked={!pruned.has(m)}
                        onCheckedChange={(checked) => setPruned((p) => toggled(p, m, !checked))}
                      />
                      {m}
                    </label>
                  ))}
                </div>
              )}
            </div>

            <div className="space-y-1.5">
              <p className="text-xs font-semibold text-muted">Tags</p>
              {tags.isError ? (
                <p className="text-xs text-faint">Tags could not be discovered for this project.</p>
              ) : tags.isLoading ? (
                <p className="text-xs text-faint">Loading tags…</p>
              ) : (tags.data ?? []).length === 0 ? (
                <p className="text-xs text-faint">No tags in use yet.</p>
              ) : (
                <div className="flex flex-wrap gap-x-4 gap-y-1.5">
                  {tags.data!.map((t) => (
                    <label key={t} className="flex items-center gap-1.5 text-xs text-text">
                      <Checkbox
                        ariaLabel={t}
                        checked={!prunedTags.has(t)}
                        onCheckedChange={(checked) => setPrunedTags((p) => toggled(p, t, !checked))}
                      />
                      {t}
                    </label>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

        {step === 1 && (
          <div className="space-y-3">
            <label className="block text-xs text-muted">
              Documentation paths
              <Textarea
                aria-label="Documentation paths"
                className="mt-1 h-20 w-full font-mono text-xs"
                placeholder={"docs/screens/**\nREADME.md"}
                value={docPathsText}
                onChange={(e) => setDocPathsText(e.target.value)}
              />
              <span className="mt-1 block text-faint">One glob or path per line, relative to the repo root.</span>
            </label>
            <label className="block text-xs text-muted">
              Team conventions
              <Textarea
                aria-label="Team conventions"
                className="mt-1 h-20 w-full"
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

        <div className="flex justify-end gap-2 pt-1">
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
