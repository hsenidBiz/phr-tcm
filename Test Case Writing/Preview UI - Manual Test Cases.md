# Preview — UI Manual Test Cases

These test cases are based on direct UI observation of the Preview screen (Step 6 of 6) in the Definition Wizard, combined with the Phase 7 ADR handler specification. Database, API, and implementation details are intentionally omitted.

Assumptions used for this test set:
- The user is logged in as an HR Admin (Sarah Thompson role observed in the UI).
- Rating Methods row is expanded by default on the observed page; other rows are collapsed.
- "Save Definitions" (body panel) stays on the page; "Save & Exit" (footer) redirects to the dashboard.

## Topics Covered

- Page load and layout display
- Step progress indicator (100% complete) and breadcrumb navigation
- "5 Complete" summary badge
- Step summary rows — collapsed and expanded states
- "Ready" status badge and Edit button per row
- Expanded step detail content (Rating Methods example)
- "Ready to Save" panel display and Save Definitions button
- Save Definitions — happy path (stay on page, success panel)
- Save Definitions — HTTP 400 validation (incomplete steps)
- Save & Exit — happy path (redirect to dashboard)
- Save & Exit — HTTP 400 validation
- Version number and timestamp in success panel
- Double-click protection on Save Definitions
- Previous button and footer navigation
- Header button behaviour (Cancel, Save)
- Edit button navigation from step rows
- Step state regression (Ready badge update after Edit)
- Rapid navigation between Edit and Preview
- Offline behaviour during Save Definitions
- Large-data rendering in expanded step details
- Role gating for the Preview step and the Save Definitions action
- Sign-out, session timeout, and concurrent-tab handling around publish
- XSS bleed-through from earlier step values into Preview's expanded detail

---

## 1. Page Load and Layout

### PV-101 — Preview page opens at Step 6 of 6 with 100% complete progress bar
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
- Expected Results:
    - The page heading reads "Definition Wizard" with subtitle "Configure your performance management system".
    - The step counter shows "Step 6 of 6".
    - The progress bar is fully filled and labelled "100% Complete".
    - The "Preview" section heading is visible in the main content area.
    - No broken layout or missing content is visible.
- Priority: High

### PV-102 — Step breadcrumb shows all prior steps complete and Preview as the active step
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Observe the step breadcrumb bar below the progress bar.
- Expected Results:
    - Proficiency Levels, Competencies, Proficiency Profile, and Goal Groups each display a filled green circle with a checkmark.
    - Step 6 (Preview) is highlighted as the current active step with a filled blue/dark circle labelled "6".
    - Navigation arrows are present at the ends of the breadcrumb for scrolling.
- Priority: High

### PV-103 — "5 Complete" badge is displayed at the top of the Preview content
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Observe the area directly below the "Preview" heading.
- Expected Results:
    - A green badge with a checkmark icon and the label "5 Complete" is visible.
    - The number matches the total count of completed steps.
- Priority: High

---

## 2. Step Summary Rows

### PV-104 — All 5 step summary rows are displayed with name, green checkmark, Ready badge, and Edit button
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Observe the list of step summary rows.
- Expected Results:
    - Five rows are visible: Rating Methods, Proficiency Levels, Competencies, Proficiency Profile, Goal Groups.
    - Each row shows:
        - An expand/collapse arrow on the left.
        - A step-specific icon.
        - A green filled checkmark icon.
        - The step name.
        - A "Ready" badge (outlined pill label).
        - An "Edit" text button on the right.
    - No row shows an error, incomplete, or warning state.
- Priority: High

### PV-105 — Rating Methods row is expanded by default and shows step summary detail
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Observe the Rating Methods row without taking any action.
- Expected Results:
    - The Rating Methods row is expanded by default, showing its detail content below the row header.
    - The detail content shows:
        - "Total Methods: 2"
        - "Active: 5-Point Performance Scale, Competency Rating Scale"
    - The expand arrow on the row points downward (collapsed arrow direction) to indicate expanded state.
- Priority: High

### PV-106 — Clicking an expanded row's arrow collapses its detail content
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the expand arrow on the Rating Methods row.
    14. Observe the row.
- Expected Results:
    - The detail content (Total Methods, Active) collapses and is no longer visible.
    - The expand arrow rotates or changes to indicate the collapsed state.
    - The row header (name, checkmark, Ready badge, Edit button) remains visible.
- Priority: Medium

### PV-107 — Clicking a collapsed row's arrow expands its detail content
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the expand arrow on the Proficiency Levels row.
    14. Observe the row.
- Expected Results:
    - The detail content for Proficiency Levels expands and becomes visible below the row header.
    - The expand arrow updates to indicate the expanded state.
- Priority: Medium

### PV-108 — Multiple rows can be expanded simultaneously
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Expand the Proficiency Levels row.
    14. Expand the Competencies row.
    15. Observe both rows.
- Expected Results:
    - Both rows are expanded simultaneously.
    - Expanding one row does not collapse another.
- Priority: Low

---

## 3. Edit Button Navigation

### PV-109 — Clicking Edit on Rating Methods navigates to the Rating Methods step
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Edit" button on the Rating Methods row.
    14. Observe where the user is taken.
- Expected Results:
    - The user is navigated to the Rating Methods step.
    - The step counter and breadcrumb update to reflect the Rating Methods step.
    - Any unsaved state on the Preview page is preserved or handled gracefully.
- Priority: High

### PV-110 — Clicking Edit on any step row navigates to that specific step
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Edit" button on the Goal Groups row.
    14. Observe where the user is taken.
- Expected Results:
    - The user is navigated to the Goal Groups step.
    - The same behaviour applies for all other Edit buttons (Proficiency Levels, Competencies, Proficiency Profile).
- Priority: High

---

## 4. Ready to Save Panel

### PV-111 — "Ready to Save" panel is displayed when all steps are complete
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Scroll to the bottom of the main content area.
    14. Observe the panel below the step summary rows.
- Expected Results:
    - A green-bordered panel is visible with the heading "Ready to Save".
    - The body text reads "All configurations are complete. You can save your definitions."
    - A "Save Definitions" button with a floppy disk icon is displayed inside the panel.
- Priority: High

### PV-112 — "Ready to Save" panel and Save Definitions button are not shown when steps are incomplete
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Edit" button on one step row to navigate to that step.
    14. Clear or invalidate the required input on that step so it is no longer in a "Ready" state.
    15. Navigate back to Step 6 (Preview) via the breadcrumb.
    16. Observe the bottom of the main content area.
- Expected Results:
    - The "Ready to Save" panel is either not displayed or the Save Definitions button is disabled/absent.
    - An alternative state (e.g., an incomplete steps warning) is shown instead.
- Priority: High

---

## 5. Save Definitions — Stay on Page

### PV-113 — Clicking Save Definitions with all steps complete publishes and shows a success panel
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Save Definitions" button in the "Ready to Save" panel.
    14. Observe the page after the request completes.
- Expected Results:
    - The setup is published successfully.
    - The user remains on the Preview page; no redirect occurs.
    - A success panel appears on the page confirming the definitions were saved.
    - The definition version number is incremented and shown in the success panel.
    - A timestamp of the save event is displayed in the success panel.
- Priority: High

### PV-114 — The success panel after Save Definitions shows the new version number and timestamp
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Save Definitions" button in the "Ready to Save" panel.
    14. Observe the success panel content.
- Expected Results:
    - The new definition version number (previous version + 1) is displayed.
    - A timestamp is shown indicating when the definitions were saved.
    - Both values are clearly formatted and readable.
- Priority: High

### PV-115 — Clicking Save Definitions when a step is incomplete returns an error toast
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Edit" button on one step row, clear or invalidate its required input, then return to Step 6 (Preview) via the breadcrumb.
    14. Confirm at least one step row now shows a non-Ready or incomplete status.
    15. Attempt to click "Save Definitions" if the button is accessible.
    16. Observe the response.
- Expected Results:
    - The server returns HTTP 400.
    - A toast message appears with the error text provided by the server.
    - The user remains on the Preview page.
    - No publish event occurs and the version number does not increment.
    - The success panel does not appear.
- Priority: High

---

## 6. Save & Exit — Redirect to Dashboard

### PV-116 — Clicking Save & Exit with all steps complete publishes and redirects to the dashboard
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Scroll to the footer.
    14. Click the "Save & Exit" button.
    15. Observe the page behaviour after the request completes.
- Expected Results:
    - The setup is published successfully.
    - The user is redirected away from the Preview page to the dashboard or relevant landing page.
    - No success panel is shown on the Preview page (the redirect replaces that behaviour).
- Priority: High

### PV-117 — Save & Exit with an incomplete step returns an error toast and no redirect occurs
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Edit" button on one step row, clear or invalidate its required input, then return to Step 6 (Preview) via the breadcrumb.
    14. Confirm at least one step row now shows a non-Ready or incomplete status.
    15. Attempt to click "Save & Exit".
    16. Observe the response.
- Expected Results:
    - The server returns HTTP 400.
    - A toast message appears with the server-provided error.
    - The user remains on the Preview page; no redirect occurs.
- Priority: High

### PV-118 — Save Definitions and Save & Exit trigger different post-publish behaviours
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click "Save Definitions" in the "Ready to Save" panel.
    14. Confirm the success panel appears and no redirect has occurred.
    15. In a subsequent session or after resetting, click "Save & Exit" from the footer.
    16. Confirm a redirect to the dashboard occurs.
- Expected Results:
    - "Save Definitions" consistently keeps the user on the page and shows the success panel.
    - "Save & Exit" consistently redirects after publish.
    - The two buttons do not swap or share behaviour.
- Priority: High

---

## 7. Version and Publish Behaviour

### PV-119 — Each successful publish increments the definition version by exactly 1
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Note the current version if shown in the preview summary.
    14. Click "Save Definitions".
    15. Observe the version in the success panel.
- Expected Results:
    - The version displayed in the success panel is exactly the previous version + 1.
    - No version is skipped or incremented by more than 1 (ADR-029).
- Priority: High

### PV-120 — Returning to Preview after a publish shows the updated version in the summary (Change)
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click "Save Definitions" to complete a successful publish.
    14. Navigate away from Preview (e.g., click a step breadcrumb).
    15. Return to the Preview step.
    16. Observe the version shown in the preview summary.
- Expected Results:
    - The preview summary displays the newly incremented version number.
    - The pre-publish version is no longer shown.
- Priority: High

### PV-121 — Rapid double-click on Save Definitions fires only one publish event
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Double-click the "Save Definitions" button rapidly.
    14. Observe the version number in the success panel.
- Expected Results:
    - Only one publish event is sent to the server.
    - The version number increments by exactly 1, not 2.
    - The button is disabled or debounced after the first click to prevent duplicate submissions.
- Priority: High

---

## 8. Footer and Header Buttons

### PV-122 — Previous button navigates back to Step 5 (Goal Groups)
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Scroll to the footer.
    14. Click the "← Previous" button.
- Expected Results:
    - The user is taken to the Goal Groups step (Step 5 of 6).
    - The step counter and breadcrumb update accordingly.
    - No data loss occurs from the Preview page.
- Priority: High

### PV-123 — Footer shows Previous and Save & Exit buttons (no Continue button on final step)
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Scroll to the footer area.
    14. Observe the available buttons.
- Expected Results:
    - The footer contains "← Previous" and "Save & Exit" buttons.
    - There is no "Continue" button, as Preview is the final step.
    - No "Save" (standalone save without exit) button is in the footer.
- Priority: Medium

### PV-124 — Cancel button in the header exits without triggering a publish
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Cancel" button in the header.
    14. Observe the result.
- Expected Results:
    - The user is taken away from the Preview step.
    - No publish event is triggered.
    - The definition version is not incremented.
    - A confirmation prompt appears if there are unsaved changes, or the navigation proceeds cleanly.
- Priority: High

### PV-125 — Save button in the header saves progress without publishing
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click the "Save" button in the page header.
    14. Observe the result.
- Expected Results:
    - The current state is saved.
    - No publish event occurs; the definition version is not incremented.
    - The user remains on the Preview page.
    - A confirmation indicator (toast or visual feedback) is shown.
- Priority: Medium

---

## 9. Negative and Edge Cases

### PV-126 — Editing a previously-Ready step to make it incomplete updates the row badge from Ready on return
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Confirm the Goal Groups row on the Preview page shows a "Ready" badge.
    14. Click "Edit" on the Goal Groups row.
    15. Toggle all goal groups to inactive (so the step is no longer in a Ready state) and save the change.
    16. Use the breadcrumb to return to Step 6 (Preview).
    17. Observe the Goal Groups row badge and the "5 Complete" summary badge.
- Expected Results:
    - The Goal Groups row no longer shows the "Ready" badge; it shows an incomplete or warning state instead.
    - The "5 Complete" summary badge updates to reflect a lower completed count.
    - The "Ready to Save" panel and Save Definitions button are no longer shown (or Save Definitions is disabled).
- Priority: High

### PV-127 — Rapid Edit-then-back navigation does not corrupt the Preview state
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Click "Edit" on the Rating Methods row, then immediately click the browser back button.
    14. Click "Edit" on the Proficiency Levels row, then click the breadcrumb to return to Step 6 (Preview).
    15. Click "Edit" on the Goal Groups row, then click the breadcrumb to return to Step 6 (Preview).
    16. Observe the Preview page.
- Expected Results:
    - All five step rows continue to render with their correct names, icons, "Ready" badges, and Edit buttons.
    - The "5 Complete" summary badge remains accurate.
    - No row is duplicated, missing, or stuck in a loading state.
    - No console errors are produced by the rapid navigation.
- Priority: Medium

### PV-128 — Save Definitions while the browser is offline shows an actionable error and does not increment the version
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Note the current version number shown in the preview summary.
    14. Disconnect the browser network (e.g., toggle offline mode in DevTools or disable the network adapter).
    15. Click the "Save Definitions" button.
    16. Reconnect the network and observe the preview summary.
- Expected Results:
    - An actionable error is shown (toast or inline message) indicating the request could not reach the server.
    - The version number in the preview summary remains unchanged (no false increment).
    - The user remains on the Preview page; no success panel appears.
    - Clicking Save Definitions again after reconnect publishes correctly with the version incrementing by exactly 1.
- Priority: High

### PV-129 — Rating Methods row with 20 or more active methods remains scrollable inside the expanded detail
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Confirm 20 or more active rating methods are configured (test data prerequisite).
    14. Ensure the Rating Methods row is expanded.
    15. Scroll within the expanded detail content.
- Expected Results:
    - The expanded detail renders all active method names without overlap or truncation errors.
    - The detail area is scrollable (or the page scrolls) without freezing.
    - The "Total Methods" count matches the number of active methods.
- Priority: Medium

### PV-130 — Preview with 50 or more goal groups remains scrollable and rendered correctly in the Goal Groups expanded detail
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure 50 or more active goal groups are configured (test data prerequisite).
    12. Click "Continue".
    13. Click the expand arrow on the Goal Groups row on the Preview page.
    14. Scroll the expanded detail.
- Expected Results:
    - All 50+ goal groups are rendered in the expanded detail without clipping, overlap, or truncation errors.
    - The detail area scrolls smoothly and the page does not freeze.
    - The summary count (where shown) matches the number of active goal groups.
- Priority: Medium

---

## 10. Security

### PV-131 — Step 6 (Preview) and Save Definitions are not reachable via the sidebar for non-HR-Admin roles
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. Open the sidebar user menu by clicking the "Sarah Thompson HR Admin" button.
    2. From the "Switch Role" submenu, click "Supervisor".
    3. Observe the sidebar navigation.
    4. Change the browser URL to `/setup/definitions` and press Enter.
    5. Observe the page that loads.
    6. Re-open the sidebar user menu and switch to "Employee", repeating steps 3–5.
    7. Switch the role back to "HR Admin".
- Expected Results:
    - As Supervisor or Employee, the "Setup & Configuration" sidebar group is not displayed, so Definition Wizard (and therefore the Preview step) is not reachable via the nav.
    - Direct navigation to `/setup/definitions` as Supervisor or Employee does not render the Preview step, the Save Definitions button, or the "Ready to Save" panel.
    - Restoring the role to HR Admin brings back the Setup & Configuration entry, and the Preview step is reachable again.
- Priority: High

### PV-132 — Signing out before clicking Save Definitions does not publish the configuration
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Note the current definition version shown in the Preview summary.
    14. Without clicking "Save Definitions", open the sidebar user menu and click "Sign Out".
    15. Sign back in as the same HR Admin user and return to Step 6 (Preview).
    16. Observe the version number in the preview summary.
- Expected Results:
    - Signing out returns the user to the sign-in screen without triggering a publish.
    - After signing back in, the definition version is unchanged (no auto-publish occurred during sign-out).
    - No success panel appears, and the "Ready to Save" panel still offers the Save Definitions button as before.
- Priority: High

### PV-133 — Inactivity past the session timeout before clicking Save Definitions requires re-authentication and does not publish
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Note the current definition version shown in the Preview summary.
    14. Leave the browser tab idle past the application's inactivity timeout.
    15. Click the "Save Definitions" button in the "Ready to Save" panel.
- Expected Results:
    - The publish attempt prompts the user to re-authenticate (redirect to sign-in, inline session-expired modal, or toast directing the user to sign in again).
    - No silent publish occurs against an expired session.
    - The definition version in the preview summary remains unchanged until a successful re-auth and a fresh publish.
- Priority: Medium

### PV-134 — Clicking Save Definitions from two browser tabs simultaneously results in at most one publish event
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Open the same Preview URL in a second browser tab so both Tab A and Tab B show Step 6 with the "Ready to Save" panel.
    14. Note the current definition version in Tab A.
    15. Click "Save Definitions" in Tab A and, within a second or two, click "Save Definitions" in Tab B before Tab A's success panel renders.
    16. After both responses settle, return to either tab and observe the version number.
- Expected Results:
    - The definition version increments by exactly 1 across the two clicks, not by 2.
    - One tab shows the success panel; the other tab either also shows the success panel referencing the same incremented version, or shows a clear toast (e.g., HTTP 400/409) explaining that the configuration was already published.
    - No duplicate publish event, version skip, or silent overwrite occurs.
- Priority: High

### PV-135 — A script payload entered as a Goal Group field on Step 5 renders as escaped text in the Goal Groups expanded detail on Preview
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), open the "+ Add Goal Group" dialog.
    12. Enter `<script>alert('xss-preview')</script>` as the Group Name and `<img src=x onerror=alert(1)>` as the Description, set valid Min/Max/Weightage values, and click "Create".
    13. Click "Continue" to reach Step 6 (Preview).
    14. Click the expand arrow on the Goal Groups row.
    15. Observe the expanded detail content.
- Expected Results:
    - No JavaScript alert dialog appears on Step 5, during navigation, or on Step 6.
    - The Goal Groups expanded detail on the Preview page shows the payload values as literal text (the `<script>...</script>` and `<img...>` characters), not as parsed HTML.
    - The Preview row layout is not broken by the payload, and no console errors are produced.
    - The "5 Complete" badge and the "Ready" badge on the Goal Groups row are unaffected by the payload values.
- Priority: High

### PV-136 (New) — Save & Exit persists data when navigating out and back to the Performance Management module
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. On the Wizard Landing page, select an industry from the "Select Your Industry" combobox.
    2. Click "Continue to Configuration".
    3. On Step 1 (Rating Methods), choose a Quick Start template or add a rating method.
    4. Click "Continue".
    5. On Step 2 (Proficiency Levels), select the proficiency levels.
    6. Click "Continue".
    7. On Step 3 (Competencies), select the competencies.
    8. Click "Continue".
    9. On Step 4 (Proficiency Profile), select a grouping type and assign competency profiles.
    10. Click "Continue".
    11. On Step 5 (Goal Groups), ensure at least one active goal group is configured.
    12. Click "Continue".
    13. Observe the Preview page at Step 6 with all steps marked as "Ready".
    14. Click the "Save & Exit" button in the footer.
    15. Observe the page where the user is taken (should be outside the wizard).
    16. Navigate to a different module or section of the application.
    17. Navigate back to the Performance Management module and re-enter the Definition Wizard.
    18. Advance to Step 6 (Preview) by clicking Continue through Steps 1–5.
    19. Observe the step summary rows and their status.
- Expected Results:
    - After clicking Save & Exit, the user exits the wizard and is taken to the dashboard or relevant landing page.
    - After navigating away and returning to the Performance Management module, the Definition Wizard retains the saved configuration state (all steps remain marked as "Ready" and the definition version may have been incremented if Save Definitions was previously clicked).
    - No data loss occurs between the exit and re-entry.
    - The step progress is retained and the breadcrumb reflects the completed steps.
    - All five step summary rows are visible with their current configuration intact.
- Priority: High

