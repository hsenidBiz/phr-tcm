import { useState } from "react";
import type { PbiHit, TestCase } from "../bindings";
import QueueSection from "../components/QueueSection";
import { Button } from "../components/ui/button";
import { Input, Textarea } from "../components/ui/input";
import { Select } from "../components/ui/select";
import { useQueue } from "../hooks/useQueue";

function parseStepsText(text: string) {
  return text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .map((line) => {
      const [action, expected = ""] = line.split("=>");
      return { action: action.trim(), expected: expected.trim() };
    })
    .filter((s) => s.action);
}

export default function ManualEntry({
  org,
  project,
  pbi,
}: {
  org: string;
  project: string;
  pbi: PbiHit | null;
}) {
  const { queue, setQueue } = useQueue(org, pbi?.id ?? null);
  const [title, setTitle] = useState("");
  const [stepsText, setStepsText] = useState("");
  const [tags, setTags] = useState("");
  const [status, setStatus] = useState("Not Automated");

  if (!org || !project || !pbi) {
    return (
      <p className="text-sm text-muted">
        Pick an organization, project and PBI in the bar above to start
        writing test cases.
      </p>
    );
  }

  function addManual() {
    const steps = parseStepsText(stepsText);
    if (!title.trim() || steps.length === 0) return;
    const tc: TestCase = {
      title: title.trim(),
      steps,
      tags: tags.trim(),
      automation_status: status,
      module_value: "",
      preconditions: "",
      update_id: null,
    };
    setQueue((q) => [...q, tc]);
    setTitle("");
    setStepsText("");
    setTags("");
  }

  return (
    <div className="space-y-4">
      <section className="max-w-2xl space-y-2 rounded-md border border-border bg-surface p-4">
        <h2 className="text-sm font-semibold text-text">New test case</h2>
        <Input
          className="w-full"
          placeholder="Test case title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <Textarea
          className="h-28 w-full font-mono text-xs"
          placeholder={"One step per line:\naction => expected result"}
          value={stepsText}
          onChange={(e) => setStepsText(e.target.value)}
        />
        <div className="flex gap-2">
          <Input
            className="flex-1"
            placeholder="Tags (semicolon-separated)"
            value={tags}
            onChange={(e) => setTags(e.target.value)}
          />
          <Select value={status} onChange={(e) => setStatus(e.target.value)}>
            <option>Not Automated</option>
            <option>Planned</option>
          </Select>
          <Button variant="outline" size="sm" onClick={addManual}>
            Add to queue
          </Button>
        </div>
      </section>

      <QueueSection org={org} project={project} pbiId={pbi.id} queue={queue} setQueue={setQueue} />
    </div>
  );
}
