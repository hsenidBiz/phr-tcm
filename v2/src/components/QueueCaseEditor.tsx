// Inline editor for one QUEUED case (imported or manually added, not yet
// submitted): the same form as the existing-case editor, but Save writes back
// into the local queue — nothing touches Azure DevOps until the review gate's
// submit. update_id is preserved, so an edited UPDATE row stays an update.

import { useState } from "react";
import type { TestCase } from "../bindings";
import { validateCase } from "../lib/validate";
import ModuleField from "./ModuleField";
import StepsEditor from "./StepsEditor";
import TagsField from "./TagsField";
import { Button } from "./ui/button";
import { Input, Textarea } from "./ui/input";
import { Select } from "./ui/select";

export default function QueueCaseEditor({
  original,
  org,
  project,
  onSave,
  onCancel,
}: {
  original: TestCase;
  org: string;
  project: string;
  onSave: (tc: TestCase) => void;
  onCancel: () => void;
}) {
  const [tc, setTc] = useState<TestCase>(original);
  const problem = validateCase(tc);

  return (
    <div className="space-y-2 border-t border-border p-3">
      <div className="flex gap-2">
        <Input
          aria-label="Case title"
          className="flex-1"
          value={tc.title}
          onChange={(e) => setTc((t) => ({ ...t, title: e.target.value }))}
        />
        <Select
          aria-label="Automation status"
          value={tc.automation_status}
          onChange={(e) => setTc((t) => ({ ...t, automation_status: e.target.value }))}
        >
          <option>Not Automated</option>
          <option>Planned</option>
        </Select>
      </div>
      <div className="flex gap-2">
        <TagsField
          org={org}
          project={project}
          className="flex-1"
          value={tc.tags}
          onChange={(v) => setTc((t) => ({ ...t, tags: v }))}
        />
        <ModuleField
          org={org}
          project={project}
          className="w-48"
          value={tc.module_value}
          onChange={(v) => setTc((t) => ({ ...t, module_value: v }))}
        />
      </div>
      <Textarea
        aria-label="Preconditions"
        className="h-16 w-full"
        placeholder="Preconditions"
        value={tc.preconditions}
        onChange={(e) => setTc((t) => ({ ...t, preconditions: e.target.value }))}
      />

      <StepsEditor steps={tc.steps} onChange={(steps) => setTc((t) => ({ ...t, steps }))} />

      <div className="flex items-center gap-3">
        <Button size="sm" disabled={Boolean(problem)} onClick={() => onSave(tc)}>
          Save to queue
        </Button>
        <Button variant="ghost" size="sm" onClick={onCancel}>
          Cancel
        </Button>
        {problem && <span className="text-xs text-danger">{problem}</span>}
      </div>
    </div>
  );
}
