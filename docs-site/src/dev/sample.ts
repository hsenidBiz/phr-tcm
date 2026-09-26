// DEV ONLY: a sample page for judging the design before real content and
// shots exist. Loaded by main.ts only under `vite dev` with `?sample` in
// the URL; production builds drop the branch, and the built-file test
// checks no "sample-" id reaches src-tauri/help/.

import { SHOT_HEIGHT, SHOT_WIDTH, type Box, type ShotPositions, type SiteContent } from "../types";
import { intro } from "../content";

const main = (controls: Record<string, Box>): ShotPositions => ({ size: { w: SHOT_WIDTH, h: SHOT_HEIGHT }, controls });

export const sample: SiteContent = {
  intro: { ...intro, quickStart: intro.quickStart.map((s, i) => ({ ...s, link: i < 5 ? "sample-import" : "sample-run" })) },
  screens: [
    {
      id: "sample-import",
      title: "Import File",
      group: "Test cases",
      summary:
        "Bring in test cases from a JSON file, check them in the queue, then upload them to the selected PBI. A case with an id updates that work item; a blank id creates a new one.",
      shots: [
        { id: "sample-import-queue", route: [], alt: "Import File with three cases in the queue" },
        { id: "sample-import-review", route: [], alt: "Import File with the review panel open" },
      ],
      controls: [
        { id: "choose-file", shot: "sample-import-queue", locate: { role: "button", name: "Choose file" }, name: "Choose file", does: "Opens a JSON file of test cases and adds them to the queue." },
        { id: "filter", shot: "sample-import-queue", locate: { label: "Filter" }, name: "Filter", does: "Narrows the queue to cases whose title or tags contain the text." },
        { id: "select-all", shot: "sample-import-queue", locate: { role: "checkbox", name: "Select all" }, name: "Select all", does: "Ticks every case in the queue, ready for a bulk action." },
        { id: "upload", shot: "sample-import-queue", locate: { role: "button", name: "Upload" }, name: "Upload", does: "Sends the ticked cases to Azure DevOps and links each one to the PBI.", tips: ["Nothing is sent until you press **Upload**."] },
        { id: "review-steps", shot: "sample-import-review", locate: { text: "Steps" }, name: "Steps", does: "Every step with its expected result, as it will appear in Azure DevOps." },
        { id: "review-save", shot: "sample-import-review", locate: { role: "button", name: "Save" }, name: "Save changes", does: "Keeps your edits to this case in the queue." },
        { id: "not-placed", shot: "sample-import-review", locate: { text: "Later" }, name: "Share link", does: "Copies a link that opens this review page in a browser." },
      ],
      tips: ["Press [[Ctrl+K]] to jump to any screen.", "A duplicate title is a warning, never an update: only the case id decides."],
      howTo: [{ title: "Update cases you already uploaded", steps: ["Export them with their ids.", "Edit the file.", "Import it again and press **Upload**."] }],
    },
    {
      id: "sample-run",
      title: "Run Tests",
      group: "Running tests",
      summary: "Pick the cases to run, open them in the runner and record a result for every step.",
      shots: [{ id: "sample-run-selected", route: [], alt: "Run Tests with two cases selected" }],
      controls: [
        { id: "suite", shot: "sample-run-selected", locate: { label: "Suite" }, name: "Suite", does: "Chooses the test suite whose cases are listed." },
        { id: "run-in-runner", shot: "sample-run-selected", locate: { role: "button", nameRe: "^Run \\d+ in runner$" }, name: "Run 2 in runner", does: "Opens the runner window with the selected cases in order." },
        { id: "outcome", shot: "sample-run-selected", locate: { text: "Outcome" }, name: "Outcome", does: "The last recorded result of each case." },
      ],
    },
    {
      id: "sample-settings",
      title: "Settings",
      group: "Settings",
      summary: "Theme, accent, notifications and the logs to attach to a bug report.",
      shots: [{ id: "sample-settings-main", route: [], alt: "Settings, Appearance section" }],
      controls: [
        { id: "nav", shot: "sample-settings-main", locate: { role: "link", name: "Settings" }, name: "Settings in the sidebar", does: "Opens this screen from anywhere in the app." },
        { id: "project", shot: "sample-settings-main", locate: { label: "Project" }, name: "Project", does: "The Azure DevOps project every screen works in." },
        { id: "theme", shot: "sample-settings-main", locate: { label: "Theme" }, name: "Theme", does: "Light, dark, or follow Windows." },
        { id: "logs", shot: "sample-settings-main", locate: { role: "button", name: "Open logs" }, name: "Open logs", does: "Shows the folder with the app's log files." },
      ],
    },
  ],
  recipes: [
    {
      id: "sample-first-upload",
      title: "Upload your first cases",
      steps: [
        { text: "Sign in and pick the PBI.", link: "sample-import" },
        { text: "Press **Choose file** and open your JSON.", link: "sample-import/choose-file" },
        { text: "Tick the cases and press **Upload**.", link: "sample-import/upload" },
      ],
    },
    {
      id: "sample-run-two",
      title: "Run two cases and record results",
      steps: [
        { text: "Pick the suite.", link: "sample-run/suite" },
        { text: "Tick two cases, then **Run 2 in runner**.", link: "sample-run/run-in-runner" },
        { text: "Mark each step passed or failed." },
      ],
    },
  ],
  positions: {
    "sample-import-queue": main({
      "choose-file": { x: 1180, y: 118, w: 150, h: 40 },
      filter: { x: 110, y: 118, w: 260, h: 40 },
      "select-all": { x: 116, y: 520, w: 24, h: 24 },
      upload: { x: 1240, y: 820, w: 130, h: 42 },
    }),
    "sample-import-review": main({
      "review-steps": { x: 110, y: 300, w: 700, h: 360 },
      "review-save": { x: 1180, y: 118, w: 150, h: 40 },
    }),
    "sample-run-selected": main({
      suite: { x: 110, y: 118, w: 260, h: 40 },
      "run-in-runner": { x: 1180, y: 118, w: 150, h: 40 },
      outcome: { x: 1100, y: 500, w: 220, h: 300 },
    }),
    "sample-settings-main": main({
      nav: { x: 0, y: 150, w: 86, h: 44 },
      project: { x: 300, y: 0, w: 220, h: 48 },
      theme: { x: 110, y: 240, w: 400, h: 150 },
      logs: { x: 1180, y: 118, w: 150, h: 40 },
    }),
  },
  available: [],
};
