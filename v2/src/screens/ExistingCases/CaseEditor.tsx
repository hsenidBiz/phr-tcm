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
import { IconConfirm, IconUndo } from "../../lib/actionIcons";

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

  /** What Discard puts back.
   *
   * Seeded from the case as it loaded, and moved FORWARD on every
   * successful save. Without that second part, an editor left open after
   * a save would still offer to revert to the pre-edit values - which
   * Azure DevOps no longer holds, so "discard" would silently become
   * "undo the thing I just saved, locally, and re-save it next time". */
  const [baseline, setBaseline] = useState<TestCase>(() => toTestCase(original));
  // Both sides are built by the same function and only ever updated by
  // spreads, so their keys stay in the same order and a string compare is
  // a sound deep compare - steps included, which a shallow one would miss.
  const dirty = JSON.stringify(tc) !== JSON.stringify(baseline);

  const discard = () => {
    const before = tc;
    setTc(baseline);
    // Discarding is one click next to Save and there is no other copy of
    // what was typed, so it comes with a way back rather than a
    // confirmation - the same bargain Power Rename makes.
    toast.info("Changes discarded.", {
      action: { label: "Undo", onClick: () => setTc(before) },
      duration: 10_000,
    });
  };

  const saveCase = useMutation({
    mutationFn: async () => {
      // Pin what is being sent. `onSuccess` runs with the LATEST render's
      // `tc`, so typing during the round trip made the success toast name a
      // title Azure DevOps does not hold and set the revert baseline to an
      // edit that was never saved - which made Discard disappear on it.
      const sent = tc;
      // `original.steps_xml` is the Steps field as Azure DevOps holds it.
      // Passing it lets the save leave Steps out of the patch when they were
      // not edited - without it, saving a case you only retitled rewrites
      // the steps from a plain-text read and strips their formatting and
      // embedded screenshots.
      const r = await commands.updateTestCase(
        org,
        project,
        sent,
        moduleRef,
        preconditionsRef,
        original.steps_xml,
      );
      if (r.status === "error") throw new Error(r.error);
      return sent;
    },
    onSuccess: (sent) => {
      toast.success(`Updated #${original.id}: ${sent.title}`);
      setBaseline(sent); // what is now in Azure DevOps is the thing to revert to
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
          <IconConfirm aria-hidden />
          {saveCase.isPending ? "Saving" : "Save changes"}
        </Button>
        {/* Only once there is something to discard. A permanently visible
            Discard invites the question "discard what?" on a case nobody
            has touched, and it would be the one enabled control on an
            editor whose Save is blocked by a validation problem. */}
        {dirty && (
          <Button
            size="sm"
            variant="outline"
            disabled={saveCase.isPending}
            onClick={discard}
          >
            <IconUndo aria-hidden />
            Discard changes
          </Button>
        )}
        {problem && <span className="text-xs text-danger">{problem}</span>}
      </div>
    </div>
  );
}
