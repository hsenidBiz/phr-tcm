import { useState } from "react";
import { type PbiHit, type Step, type TestCase } from "../bindings";
import ModuleField from "../components/ModuleField";
import PickPbiEmpty from "../components/PickPbiEmpty";
import QueueSection from "../components/QueueSection";
import StepsEditor from "../components/StepsEditor";
import TagsField from "../components/TagsField";
import { Button } from "../components/ui/button";
import { Input, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { useQueue } from "../hooks/useQueue";

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
  const [steps, setSteps] = useState<Step[]>([{ action: "", expected: "" }]);
  const [tags, setTags] = useState("");
  const [status, setStatus] = useState("Not Automated");
  const [moduleValue, setModuleValue] = useState("");
  const [preconditions, setPreconditions] = useState("");

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

  const cleanSteps = steps.filter((s) => s.action.trim());

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
    };
    setQueue((q) => [...q, tc]);
    setTitle("");
    setSteps([{ action: "", expected: "" }]);
    setTags("");
    setModuleValue("");
    setPreconditions("");
  }

  return (
    <div className="space-y-4">
      <section className="space-y-3 rounded-md border border-border bg-surface p-4">
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
              />
              <Select value={status} onChange={(e) => setStatus(e.target.value)}>
                <option>Not Automated</option>
                <option>Planned</option>
              </Select>
            </div>
            <ModuleField
              org={org}
              project={project}
              value={moduleValue}
              onChange={setModuleValue}
              className="w-full"
            />
            <Textarea
              className="h-20 w-full"
              placeholder="Preconditions (optional)"
              value={preconditions}
              onChange={(e) => setPreconditions(e.target.value)}
            />
          </div>
          <div className="space-y-2">
            <p className="text-xs font-medium text-muted">Steps</p>
            <StepsEditor steps={steps} onChange={setSteps} />
          </div>
        </div>
        <Button disabled={!title.trim() || cleanSteps.length === 0} onClick={addManual}>
          Add to queue
        </Button>
      </section>

      <QueueSection org={org} project={project} pbiId={pbi.id} queue={queue} setQueue={setQueue} />
    </div>
  );
}
