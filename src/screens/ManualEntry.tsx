import { useEffect, useState } from "react";
import { type PbiHit, type Step, type TestCase } from "../bindings";
import { CaseTextField, PreviewToggle } from "../components/CaseText";
import DefaultTagsDialog from "../components/DefaultTagsDialog";
import ModuleField from "../components/ModuleField";
import PickPbiEmpty from "../components/PickPbiEmpty";
import PrioritySelect, { DEFAULT_PRIORITY } from "../components/PrioritySelect";
import QueueSection from "../components/QueueSection";
import StepsEditor from "../components/StepsEditor";
import TagsField from "../components/TagsField";
import { Button } from "../components/ui/button";
import { Input } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { joinTags, splitTags } from "../components/ui/tagfield";
import { useQueue } from "../hooks/useQueue";
import { IconAdd, IconSetDefault } from "../lib/actionIcons";
import { loadDefaultTags } from "../lib/defaultTags";

/** A fresh case opens with this many blank steps, so the usual case needs no
 * "Add Step" clicks. */
const STARTING_STEPS = 3;
const blankSteps = (): Step[] =>
  Array.from({ length: STARTING_STEPS }, () => ({ action: "", expected: "" }));

export default function ManualEntry({
  org,
  project,
  pbi,
  onPickPbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
  onPickPbi?: (pbi: PbiHit) => void;
}) {
  const { queue, setQueue } = useQueue(org, pbi?.id ?? null);
  const [title, setTitle] = useState("");
  const [steps, setSteps] = useState<Step[]>(blankSteps);
  // Tags come in two halves. The project's defaults are FIXED: they ride
  // on every case and the field refuses to remove them, so the only way to
  // change them is the dialog that owns them. `extras` is what this one
  // case adds on top, and it is the only half a queued case clears.
  //
  // The effect re-seeds on a project switch, since the initializer only
  // ran for the first one - and drops the extras with it, because a tag
  // typed for one project means nothing in the next.
  const [defaults, setDefaults] = useState(() => loadDefaultTags(org, project));
  const [extras, setExtras] = useState("");
  const [editingDefaults, setEditingDefaults] = useState(false);
  const [status, setStatus] = useState("Not Automated");
  const [priority, setPriority] = useState<number>(DEFAULT_PRIORITY);
  const [moduleValue, setModuleValue] = useState("");
  const [preconditions, setPreconditions] = useState("");
  const [previewPre, setPreviewPre] = useState(false);
  useEffect(() => {
    setDefaults(loadDefaultTags(org, project));
    setExtras("");
  }, [org, project]);

  if (!org || !project || !pbi) {
    return (
      <PickPbiEmpty
        message="Pick an organization, project and PBI in the bar above to start writing test cases."
        org={org}
        project={project}
        onPickPbi={onPickPbi}
      />
    );
  }

  // A step is skipped only when BOTH halves are empty. One with just an
  // Expected Result stays so the review gate flags the missing action
  // instead of silently losing what was typed.
  const cleanSteps = steps.filter((s) => s.action.trim() || s.expected.trim());

  const lockedTags = splitTags(defaults);
  const tags = joinTags([...lockedTags, ...splitTags(extras)]);
  /** Whatever the field hands back, minus the defaults. Keeping them out
   * of `extras` is what lets a queued case clear the extras without
   * touching the set the project promised. */
  const setTags = (next: string) => {
    const fixed = lockedTags.map((t) => t.toLowerCase());
    setExtras(joinTags(splitTags(next).filter((t) => !fixed.includes(t.toLowerCase()))));
  };

  function addManual() {
    if (!title.trim() || cleanSteps.length === 0) return;
    const tc: TestCase = {
      title: title.trim(),
      steps: cleanSteps,
      tags: tags.trim(),
      automation_status: status,
      module_value: moduleValue.trim(),
      preconditions: preconditions.trim(),
      update_id: null,
      priority,
    };
    setQueue((q) => [...q, tc]);
    setTitle("");
    setSteps(blankSteps());
    // Only the extras go. The defaults stay because the next case wants
    // them too - and because clearing them here would quietly undo a
    // project setting from a form that is not allowed to change it.
    setExtras("");
    setModuleValue("");
    setPreconditions("");
  }

  return (
    <div className="space-y-4">
      <section data-tour="case-form" className="space-y-3 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">New test case</h2>
        <div className="grid gap-4 lg:grid-cols-2">
          <div className="space-y-2">
            <Input
              className="w-full"
              placeholder="Test case title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
            />
            <div className="flex gap-2">
              <TagsField
                org={org}
                project={project}
                className="flex-1"
                value={tags}
                onChange={setTags}
                locked={lockedTags}
              />
              <Select aria-label="Automation status" value={status} onChange={(e) => setStatus(e.target.value)}>
                <option>Not Automated</option>
                <option>Planned</option>
              </Select>
              <PrioritySelect value={priority} onChange={(p) => setPriority(p ?? DEFAULT_PRIORITY)} />
            </div>
            <Button variant="ghost" size="sm" onClick={() => setEditingDefaults(true)}>
              <IconSetDefault aria-hidden />
              Default tags
            </Button>
            <ModuleField
              org={org}
              project={project}
              value={moduleValue}
              onChange={setModuleValue}
              className="w-full"
            />
            <div className="space-y-1">
              <CaseTextField
                kind="prereq"
                preview={previewPre}
                aria-label="Preconditions"
                className="h-20 w-full"
                placeholder="Preconditions (optional)"
                value={preconditions}
                onChange={setPreconditions}
              />
              <PreviewToggle what="preconditions" on={previewPre} onToggle={() => setPreviewPre((p) => !p)} />
            </div>
          </div>
          <div className="space-y-2">
            <p className="text-xs font-medium text-muted">Steps</p>
            <StepsEditor steps={steps} onChange={setSteps} org={org} />
          </div>
        </div>
        <Button disabled={!title.trim() || cleanSteps.length === 0} onClick={addManual}>
          <IconAdd aria-hidden />
          Add to queue
        </Button>
      </section>

      <div data-tour="queue">
        <QueueSection org={org} project={project} pbiId={pbi.id} queue={queue} setQueue={setQueue} />
      </div>

      {editingDefaults && (
        <DefaultTagsDialog
          org={org}
          project={project}
          onClose={() => setEditingDefaults(false)}
          onSaved={setDefaults}
        />
      )}
    </div>
  );
}
