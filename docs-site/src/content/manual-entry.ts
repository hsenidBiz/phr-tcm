// Manual Entry: the form for writing one test case by hand, and the
// Default tags window it opens. The queue below the form is the same
// QueueSection Import File shows, and is documented there.
//
// Locates come from ManualEntry.tsx ("Automation status"), StepsEditor.tsx
// ("Reorder step N", "Step N action/expected", "Remove step N"), TagsField ("Tags"),
// ModuleField ("Module") and DefaultTagsDialog.tsx. The title and
// preconditions inputs have no label, so their placeholder is their name.

import type { Screen } from "../types";

const FORM = "manual-entry-form";
const DEFAULTS = "manual-entry-default-tags";

export const manualEntry: Screen = {
  id: "manual-entry",
  title: "Manual Entry",
  group: "Test cases",
  summary:
    "Write a test case by hand: a title, its steps and the optional fields, then add it to the queue. " +
    "The queue is the same one Import File shows, so cases you write and cases you import are reviewed and uploaded together.",
  shots: [
    { id: FORM, route: [{ nav: "Manual Entry" }], alt: "Manual Entry with an empty form and three cases in the queue" },
    {
      id: DEFAULTS,
      route: [{ nav: "Manual Entry" }, { click: { role: "button", name: "Default tags" } }, { waitFor: { role: "textbox", name: "Default tags" } }],
      alt: "The Default tags window",
    },
  ],
  controls: [
    {
      id: "title",
      shot: FORM,
      locate: { role: "textbox", name: "Test case title" },
      name: "Test case title",
      does: "The title, as it will appear in Azure DevOps.",
    },
    {
      id: "tags",
      shot: FORM,
      locate: { role: "textbox", name: "Tags" },
      name: "Tags",
      does:
        "Tags for this case. Type to search the project's existing tags, or type a new one. The project's default tags are already in place and cannot be removed here.",
    },
    {
      id: "automation-status",
      shot: FORM,
      locate: { role: "combobox", name: "Automation status" },
      name: "Automation status",
      does: "**Not Automated** or **Planned**. It stays as you set it for the next case.",
    },
    {
      id: "default-tags",
      shot: FORM,
      locate: { role: "button", name: "Default tags" },
      name: "Default tags",
      does: "Opens the window where you set the tags every new case in this project should carry.",
    },
    {
      id: "module",
      shot: FORM,
      locate: { role: "combobox", name: "Module" },
      name: "Module",
      does: "The module the case belongs to. Pick one from the project's list when it has one, or type your own value.",
    },
    {
      id: "preconditions",
      shot: FORM,
      locate: { role: "textbox", name: "Preconditions (optional)" },
      name: "Preconditions",
      does: "What must be true before the first step, for example an account that already exists.",
    },
    {
      id: "reorder-step",
      shot: FORM,
      locate: { role: "button", name: "Reorder step 1" },
      name: "Drag handle",
      does: "Drag a step by its handle to move it. Or click the handle and use the up and down arrow keys to move it one place at a time.",
    },
    {
      id: "step-action",
      shot: FORM,
      locate: { role: "textbox", name: "Step 1 action" },
      name: "Action",
      does: "What the tester does in this step. A step needs an action; empty ones are left out.",
    },
    {
      id: "step-expected",
      shot: FORM,
      locate: { role: "textbox", name: "Step 1 expected" },
      name: "Expected result",
      does: "What should happen after the action. It can be left empty.",
    },
    {
      id: "remove-step",
      shot: FORM,
      locate: { role: "button", name: "Remove step 1" },
      name: "Remove step (x)",
      does: "Removes the step.",
    },
    {
      id: "add-step",
      shot: FORM,
      locate: { role: "button", name: "Add Step" },
      name: "Add Step",
      does: "Adds an empty step at the end.",
    },
    {
      id: "add-to-queue",
      shot: FORM,
      locate: { role: "button", name: "Add to queue" },
      name: "Add to queue",
      does:
        "Adds the case to the queue below and clears the form for the next one. It is greyed out until the case has a title and at least one step with an action.",
      tips: ["Your default tags and the automation status stay filled in for the next case."],
    },
    {
      id: "queue",
      shot: FORM,
      locate: { text: "Queue for PBI #1001 (3 queued)" },
      name: "Queue",
      does: "The cases waiting to be uploaded to this PBI. It is the same queue as on Import File, where every part of it is explained.",
    },
    {
      id: "default-tags-field",
      shot: DEFAULTS,
      locate: { role: "textbox", name: "Default tags" },
      name: "Default tags",
      does: "The tags every new case in this project should carry. Saving an empty field removes them.",
    },
    {
      id: "default-tags-cancel",
      shot: DEFAULTS,
      locate: { role: "button", name: "Cancel" },
      name: "Cancel",
      does: "Closes the window without changing anything.",
    },
    {
      id: "default-tags-save",
      shot: DEFAULTS,
      locate: { role: "button", name: "Save" },
      name: "Save",
      does: "Saves the tags. They appear on the form at once, fixed in place, and go on every case you add from then on.",
    },
  ],
  tips: [
    "Nothing reaches Azure DevOps until you review the queue and confirm the upload on Import File or here.",
    "A case written here is always created new. To change a case already in Azure DevOps, use Update Test Cases, or import a file that carries its id.",
    "The queue is kept on this computer for each PBI, so closing the app never loses it.",
  ],
  howTo: [
    {
      title: "Write a case and upload it",
      steps: [
        "Choose the PBI in the bar at the top.",
        "Type the title, then the first step's action and expected result.",
        "Press **Add Step** for each further step, and fill in the tags, module and preconditions if you need them.",
        "Press **Add to queue**. Repeat for the next case.",
        "Press **Review** under the queue, check the cases, then confirm the upload.",
      ],
    },
  ],
};
