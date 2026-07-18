// Inline editor for one existing case (title, steps, tags, module,
// preconditions) - saves via update_test_case with blank-skip semantics.

import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { toast } from "sonner";
import { commands, type TestCase, type TestCaseFull } from "../../bindings";
import ModuleField from "../../components/ModuleField";
import StepsEditor from "../../components/StepsEditor";
import TagsField from "../../components/TagsField";
import { Button } from "../../components/ui/button";
import { Input, Textarea } from "../../components/ui/input";
import { Select } from "../../components/ui/select";
import { toTestCase } from "../../lib/testCaseConvert";
import { validateCase } from "../../lib/validate";

export default function CaseEditor({
  original,
  org,
  project,
  moduleRef,
  preconditionsRef,
  onSaved,
}: {
  original: TestCaseFull;
  org: string;
  project: string;
  moduleRef: string | null;
  preconditionsRef: string | null;
  onSaved: () => void;
}) {
  const [tc, setTc] = useState<TestCase>(() => toTestCase(original));
  const problem = validateCase(tc);

  const saveCase = useMutation({
    mutationFn: async () => {
      const r = await commands.updateTestCase(org, project, tc, moduleRef, preconditionsRef);
      if (r.status === "error") throw new Error(r.error);
    },
    onSuccess: () => {
      toast.success(`Updated #${original.id}: ${tc.title}`);
      onSaved();
    },
    onError: (e) => toast.error(`Save failed: ${e.message}`),
  });

  return (
    <div className="space-y-2 border-t border-border p-3" onClick={(e) => e.stopPropagation()}>
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
        {moduleRef && (
          <ModuleField
            org={org}
            project={project}
            className="w-48"
            value={tc.module_value}
            onChange={(v) => setTc((t) => ({ ...t, module_value: v }))}
          />
        )}
      </div>
      {preconditionsRef && (
        <Textarea
          aria-label="Preconditions"
          className="h-16 w-full"
          placeholder="Preconditions"
          value={tc.preconditions}
          onChange={(e) => setTc((t) => ({ ...t, preconditions: e.target.value }))}
        />
      )}

      <StepsEditor steps={tc.steps} onChange={(steps) => setTc((t) => ({ ...t, steps }))} />

      <div className="flex items-center gap-3">
        <Button size="sm" disabled={Boolean(problem) || saveCase.isPending} onClick={() => saveCase.mutate()}>
          {saveCase.isPending ? "Saving" : "Save changes"}
        </Button>
        {problem && <span className="text-xs text-danger">{problem}</span>}
      </div>
    </div>
  );
}
