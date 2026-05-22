# Goal Groups — UI Manual Test Cases

These test cases are based on direct UI observation of the Goal Groups screen (Step 5 of 6) in the Definition Wizard, combined with the Phase 6 ADR handler specification. Database, API, and implementation details are intentionally omitted.

Assumptions used for this test set:
- The user is logged in as an HR Admin (Sarah Thompson role observed in the UI).
- The wizard setup includes Goals / KPIs as a selected evaluation component.
- The page loads with 3 existing active goal groups pre-populated (Business Objectives, Personal Development, Team Collaboration).

## Topics Covered

- Page load and layout display
- Step progress indicator and breadcrumb navigation
- Summary row and Active badge count
- Quick Start Templates section and template application
- Search bar behaviour
- Add Goal Group button and manual creation
- Group list row display (goal range, weightage, status, toggle, edit, delete)
- Toggle switch (active/inactive) behaviour
- Edit group behaviour
- Delete group behaviour
- Row expand behaviour
- Save Progress (Continue) — happy path and HTTP 400 validation scenarios
- Footer and header button behaviour
- Version and timestamp behavior (Change) — version increment on data change only, timestamp culture formatting
- Duplicate-name and whitespace handling on add or edit
- Boundary validation for min and max goals
- Weightage (%) field validation including the <100% rule
- Rapid input and large-list scrolling
- Search behaviour with regex-like special characters
- Browser refresh and back-button behaviour
- Role gating for the Goal Groups step and sidebar entry
- XSS payload rendering in Group Name, Description, and the search bar
- Sign-out, session timeout, and concurrent-tab handling for Goal Group edits

---

## 1. Page Load and Layout

### GG-101 — Goal Groups page opens at Step 5 of 6 with the correct layout
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
- Expected Results:
    - The page heading reads "Definition Wizard" with subtitle "Configure your performance management system".
    - The step counter shows "Step 5 of 6".
    - The progress bar shows 83% Complete.
    - The "Goal Groups" section heading is visible.
    - Quick Start Templates, the group list, search bar, and Add Goal Group button are all visible.
    - Footer contains Previous, Continue, Save, and Save & Exit buttons.
    - Header contains Cancel and Save buttons.
    - No broken layout or missing content is visible.
- Priority: High

### GG-102 — Step breadcrumb shows completed steps with green checkmarks
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
    11. Observe the step breadcrumb bar below the progress bar.
- Expected Results:
    - Proficiency Levels, Competencies, and Proficiency Profile each display a filled green circle with a checkmark.
    - Step 5 (Goal Groups) is highlighted as the current active step with a filled blue circle.
    - Step 6 (Preview) is visible but not yet marked complete.
    - Navigation arrows are present at the ends of the breadcrumb for scrolling.
- Priority: High

### GG-103 — Summary row shows "Goal Groups" label, description, and Active count badge
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
    11. Observe the summary row directly below the "Goal Groups" heading.
- Expected Results:
    - The row shows the label "Goal Groups" and the description "Define categories for organizing employee goals".
    - A green badge shows the number of currently active groups (e.g., "3 Active").
    - The count matches the number of groups shown with a green Active status in the list.
- Priority: High

### GG-104 — "Templates recommended for your industry" badge is displayed
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
    11. Observe the area near the "Goal Groups" heading.
- Expected Results:
    - A badge labelled "Templates recommended for your industry" is visible alongside the heading.
    - The badge includes a decorative spark icon.
    - The badge is informational only and not interactive.
- Priority: Medium

---

## 2. Quick Start Templates

### GG-105 — All 4 Quick Start Template cards are displayed with correct labels and metadata
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
    11. Observe the "Quick Start Templates" section.
- Expected Results:
    - The section heading reads "Quick Start Templates".
    - Four template cards are displayed: Financial, Customer, Internal Processes, Learning & Growth.
    - Each card shows a green icon, a name, a one-line description, and a goal range:
        - Financial: "Revenue, profitability, and cost management goals" — 2-5 goals
        - Customer: "Customer satisfaction, retention, and acquisition goals" — 2-4 goals
        - Internal Processes: "Operational efficiency and process improvement goals" — 2-5 goals
        - Learning & Growth: "Employee development and organizational capability goals" — 2-4 goals
- Priority: High

### GG-106 — Clicking a template card creates a new group from the template's fields
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
    11. Click the Financial template card.
    12. Observe the group list.
- Expected Results:
    - A new group row is added to the list with the Financial template's name, description, and goal range pre-filled.
    - The new group is set to active by default (toggle on, Active status badge).
    - The "Active" count in the summary badge increments by 1.
    - The action completes without error.
- Priority: High

### GG-107 — Applying the same template twice creates two independent group rows
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
    11. Click the Financial template card.
    12. Click the Financial template card again.
    13. Observe the group list.
- Expected Results:
    - Two separate group rows appear with Financial template values.
    - They are independent: editing or deleting one does not affect the other.
    - The Active count badge increments by 2.
- Priority: Medium

### GG-108 — All 4 templates can be applied in sequence without error
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
    11. Click each of the 4 template cards in sequence: Financial, Customer, Internal Processes, Learning & Growth.
    12. Observe the group list after each application.
- Expected Results:
    - Each template creates a new group row.
    - All 4 groups appear in the list without error.
    - The Active count badge reflects the total active groups.
- Priority: Medium

---

## 3. Search Bar

### GG-109 — Search bar filters the group list by name in real time
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
    11. Click the search bar labelled "Search goal groups...".
    12. Type "Business".
    13. Observe the group list.
- Expected Results:
    - Only groups with "Business" in their name are shown (e.g., Business Objectives).
    - Groups that do not match are hidden from the list.
    - The filter is applied without requiring a submit action.
- Priority: High

### GG-110 — Clearing the search bar restores the full group list
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
    11. Type a search term in the search bar.
    12. Clear the search bar (delete the text or click a clear icon if present).
    13. Observe the group list.
- Expected Results:
    - All groups are shown again after the search bar is cleared.
    - No groups are hidden when the search is empty.
- Priority: Medium

### GG-111 — Searching for a term with no matching groups shows an empty search result
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
    11. Type a term that matches no group name (e.g., "ZZZNOTEXIST") into the search bar.
    12. Observe the group list area.
- Expected Results:
    - No group rows are shown.
    - A clear empty result message or indicator is displayed.
    - No error or broken layout is visible.
- Priority: Medium

---

## 4. Add Goal Group

### GG-112 — Clicking Add Goal Group opens the Create Goal Group dialog with all fields
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
    11. Click the "+ Add Goal Group" button.
    12. Observe the dialog that appears.
- Expected Results:
    - A dialog titled "Create Goal Group" opens.
    - Input fields are available for: Group Name, Description, Min Goals, Max Goals, and Weightage (%).
    - Helper text below Weightage (%) reads "Each goal group's weightage must be less than 100%."
    - Default values shown: Min Goals 1, Max Goals 5, Weightage (%) 0.
    - Buttons present: Cancel, Create, and a Close (×) icon at the top-right.
    - No is_active toggle appears in the create form (active state is managed separately).
- Priority: High

### GG-113 — Submitting a valid new group via the Create button adds it to the group list
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
    11. Click the "+ Add Goal Group" button.
    12. Enter a Group Name (e.g., "Innovation").
    13. Enter a Description (e.g., "Goals related to new ideas and product development").
    14. Set Min Goals to 1 and Max Goals to 4.
    15. Set Weightage (%) to a valid value below 100 (e.g., 10).
    16. Click the "Create" button.
    17. Observe the group list.
- Expected Results:
    - The new group appears in the list with the entered values.
    - The group shows an Active status badge and toggle by default.
    - The Active count badge in the summary row increments by 1.
    - No error is shown.
- Priority: High

### GG-114 — Clicking Create with required fields empty is blocked
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
    11. Click the "+ Add Goal Group" button.
    12. Leave the Group Name field blank but fill in a valid Description, Min Goals, Max Goals, and Weightage (%).
    13. Click the "Create" button.
    14. Observe the response.
- Expected Results:
    - The submission is blocked.
    - A validation message identifies the required field(s) (at minimum, Group Name).
    - No group is created.
- Priority: High

---

## 5. Group List Row Display

### GG-115 — Each group row displays name, description, goal range, weightage, status, toggle, edit, and delete
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
    11. Observe a single group row (e.g., Business Objectives).
- Expected Results:
    - The row shows an expand arrow on the left.
    - A circular icon is shown next to the group name.
    - The group name and description are clearly visible:
        - Business Objectives: "Key business and financial goals"
    - A goal range badge is shown (e.g., "3-5 goals").
    - A weightage badge is shown (e.g., "50% weightage").
    - A green "Active" status badge is visible.
    - A toggle switch in the on/green position is visible.
    - An edit (pencil) icon button is present.
    - A delete (trash) icon button is present in red.
- Priority: High

### GG-116 — Clicking the expand arrow on a group row reveals additional group details
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
    11. Click the expand arrow on the left of a group row (e.g., Business Objectives).
    12. Observe the expanded content.
- Expected Results:
    - The row expands to show additional details for that group.
    - The expand arrow changes direction (rotates or changes to a collapse arrow).
    - Clicking the arrow again collapses the row back to its summary state.
- Priority: Medium

---

## 6. Toggle Switch (Active/Inactive)

### GG-117 — Toggling a group to inactive updates the row status and decrements the Active badge
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
    11. Note the current Active count in the summary badge (e.g., 3 Active).
    12. Click the toggle switch on the Team Collaboration row to turn it off.
    13. Observe the row and the summary badge.
- Expected Results:
    - The toggle switch moves to the off/grey position.
    - The "Active" status badge on the row changes to reflect inactive state (e.g., badge turns grey or label changes to "Inactive").
    - The Active count in the summary badge decrements by 1 (e.g., becomes 2 Active).
    - The change persists without requiring a page action.
- Priority: High

### GG-118 — Toggling a group back to active updates the row status and increments the Active badge
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
    11. Click the toggle switch on a group row to turn it off, so at least one inactive group exists.
    12. Click the toggle switch on the now-inactive group row to turn it back on.
    13. Observe the row and the summary badge.
- Expected Results:
    - The toggle switch moves to the on/green position.
    - The "Active" status badge on the row updates to reflect active state.
    - The Active count in the summary badge increments by 1.
- Priority: High

### GG-119 — Toggling all groups to inactive then attempting Continue shows a toast error (ADR-026)
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
    11. Toggle all 3 group rows to inactive (all toggles off).
    12. Observe the Active count badge (should show 0 Active).
    13. Click the Continue button.
    14. Observe the response.
- Expected Results:
    - Toggles persist immediately (ADR-026: validation runs after toggle persistence).
    - Active count badge shows 0 Active.
    - Continue triggers a SaveProgress request that returns HTTP 400.
    - A toast message appears with a server-provided error (e.g., "At least one active goal group is required").
    - The user remains on the Goal Groups page.
    - The step is not marked complete.
- Priority: High

---

## 7. Edit Group

### GG-120 — Clicking the edit (pencil) icon opens the Edit Goal Group dialog pre-filled with current values
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
    11. Click the pencil icon on the Business Objectives row.
    12. Observe the dialog that opens.
- Expected Results:
    - A dialog titled "Edit Goal Group" opens pre-filled with the group's current values: Group Name "Business Objectives", Description "Key business and financial goals", Min Goals 3, Max Goals 5, Weightage (%) 50.
    - The dialog contains fields for Group Name, Description, Min Goals, Max Goals, and Weightage (%).
    - Helper text below Weightage (%) reads "Each goal group's weightage must be less than 100%."
    - Buttons present: Cancel, Update, and a Close (×) icon at the top-right.
    - There is no is_active toggle or checkbox in the edit form (active state is managed via the separate toggle on the row).
- Priority: High

### GG-121 — Saving valid edits via the Update button updates the group row with the new values
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
    11. Click the pencil icon on the Business Objectives row.
    12. Change the Group Name to "Strategic Business Objectives".
    13. Change the Description to "Core business and strategic financial goals".
    14. Click the "Update" button.
    15. Observe the group row.
- Expected Results:
    - The group row updates to show the new name and description.
    - The goal range and weightage remain unchanged.
    - The active status and toggle are unaffected.
    - No error is shown.
- Priority: High

### GG-122 — Saving an edit with min goals greater than max goals is rejected
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
    11. Click the pencil icon on any group row.
    12. Set Min Goals to 10 and Max Goals to 2.
    13. Click the "Update" button.
    14. Observe the response.
- Expected Results:
    - The save is rejected with a validation message explaining the invalid goal range.
    - The group row retains its original values.
- Priority: High

### GG-123 — Cancelling the edit dialog discards changes and the group row is unchanged
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
    11. Click the pencil icon on any group row.
    12. Change the Group Name to a new value.
    13. Click "Cancel" or the Close (×) icon to close the dialog without saving.
    14. Observe the group row.
- Expected Results:
    - The group row still shows the original name.
    - No changes are persisted.
- Priority: Medium

---

## 8. Delete Group

### GG-124 — Clicking the delete (trash) icon removes the group and updates the Active badge
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
    11. Note the Active count in the summary badge.
    12. Click the red trash icon on the Personal Development row.
    13. Confirm the deletion if a confirmation dialog appears.
    14. Observe the group list and summary badge.
- Expected Results:
    - The Personal Development group is removed from the list.
    - The Active count badge decrements by 1.
    - No error is shown.
- Priority: High

### GG-125 — A confirmation step or dialog is shown before deleting a group
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
    11. Click the trash icon on any group row.
    12. Observe whether a confirmation step appears before the deletion is executed.
- Expected Results:
    - A confirmation prompt, dialog, or inline confirmation appears before the group is deleted.
    - Cancelling the confirmation leaves the group intact.
    - Confirming proceeds with the deletion.
- Priority: High

### GG-126 — Deleting the last remaining group leaves the list in an empty state
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
    11. Delete groups until exactly one group remains in the list.
    12. Delete the last group.
    13. Observe the group list area.
- Expected Results:
    - The group list shows an empty state (no rows visible).
    - The Active count badge shows 0 Active.
    - The search bar and Add Goal Group button remain visible.
- Priority: High

---

## 9. Save Progress and Validation

### GG-127 — Continue succeeds when at least one active group has a valid name and goal range
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
    11. Confirm at least one group row shows Active status with a valid goal range (min ≤ max).
    12. Click the Continue button.
    13. Observe the result.
- Expected Results:
    - SaveProgress returns success.
    - The Goal Groups step is marked as complete in the breadcrumb (green checkmark).
    - The user is taken to Step 6 (Preview).
    - The step counter updates to "Step 6 of 6".
- Priority: High

### GG-128 — Continue with all groups inactive shows a toast error
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
    11. Toggle all groups to inactive.
    12. Click Continue.
    13. Observe the response.
- Expected Results:
    - HTTP 400 is returned.
    - A toast message appears with the server-provided error.
    - The user remains on the Goal Groups page.
    - The step is not marked complete.
- Priority: High

### GG-129 — Continue with no groups created at all shows a toast error
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
    11. Delete all groups until the list is empty.
    12. Click Continue.
    13. Observe the response.
- Expected Results:
    - HTTP 400 is returned.
    - A toast message appears.
    - The user remains on the Goal Groups page.
- Priority: High

### GG-130 — Fixing the validation issue and clicking Continue again succeeds
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
    11. Toggle all groups to inactive (or delete all groups) to put the page in an invalid state.
    12. Click Continue and confirm a toast error appears.
    13. Address the issue shown in the toast (e.g., activate a group or add a new one).
    14. Click Continue again.
    15. Observe the result.
- Expected Results:
    - SaveProgress returns success.
    - The step is marked complete.
    - The user advances to Step 6 (Preview).
- Priority: High

---

## 10. Footer and Header Button Behaviour

### GG-131 — Previous button navigates back to Step 4 (Proficiency Profile)
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
    11. Click the "← Previous" button.
- Expected Results:
    - The user is taken to the Proficiency Profile step (Step 4 of 6).
    - The step counter and breadcrumb update accordingly.
- Priority: High

### GG-132 — Save & Exit saves the current state and exits the wizard
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
    11. Click the "Save & Exit" button.
    12. Observe where the user is taken.
- Expected Results:
    - The current state is saved.
    - The user exits the wizard and is taken to the dashboard or relevant landing page.
    - Progress is retained for the next session.
- Priority: High

---

## 11. Version and Timestamp Behavior (Change)

### GG-133 — Version number remains unchanged when user navigates away and back without making changes
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
    11. Note the current version number displayed on the Goal Groups page (if visible in the UI header or metadata area).
    12. Click the "Previous" button to navigate to Step 4 (Proficiency Profile).
    13. Click the "Continue" button to navigate back to Step 5 (Goal Groups).
    14. Observe the version number.
    15. Click the "Save" button without making any changes to the goal groups.
    16. Observe the version number again.
- Expected Results:
    - The version number remains the same as noted in step 11 after navigating away and back.
    - The version number does not increment when Save is clicked with no data changes.
    - The version only increments when actual data modifications are made and saved.
- Priority: High

### GG-134 — Timestamp updates according to the selected culture or locale
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
    11. Note the timestamp format displayed on the Goal Groups page.
    12. Add or modify a goal group and click "Save".
    13. Observe the timestamp of the last saved action.
    14. Change the application locale/culture setting (if available) to a different culture (e.g., from en-US to de-DE or fr-FR).
    15. Observe the timestamp format on the Goal Groups page.
- Expected Results:
    - The timestamp updates and reflects the selected culture's date/time format.
    - The same timestamp value displays in the format appropriate to the selected culture (e.g., MM/DD/YYYY for en-US vs DD.MM.YYYY for de-DE).
    - No data loss or stale timestamp persists when the culture changes.
- Priority: Medium

---

## 12. Negative and Edge Cases

### GG-135 — Adding a goal group with a name that duplicates an existing group is blocked or flagged
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
    11. Click the "+ Add Goal Group" button.
    12. Enter a Group Name that exactly matches an existing group's name (e.g., "Business Objectives").
    13. Enter a valid Description, Min Goals, Max Goals, and Weightage (%).
    14. Click the "Create" button.
    15. Observe the response.
- Expected Results:
    - The submission is blocked or a clear validation message identifies the duplicate name.
    - No second group with the duplicate name is added to the list.
    - The Active count badge does not change.
- Priority: High

### GG-136 — Leading and trailing whitespace in a goal group name is trimmed or rejected on save
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
    11. Click the "+ Add Goal Group" button.
    12. Enter "  Innovation  " (with leading and trailing spaces) as the Group Name.
    13. Enter a valid Description, Min Goals, Max Goals, and Weightage (%).
    14. Click the "Create" button.
    15. Observe the saved row.
- Expected Results:
    - The saved group name is trimmed of leading and trailing whitespace and displays as "Innovation", or
    - The submission is rejected with a clear validation message about whitespace.
    - No row appears with leading or trailing whitespace in its name.
- Priority: Medium

### GG-137 — A very long goal group name is truncated in the row with a tooltip on hover
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
    11. Click the "+ Add Goal Group" button.
    12. Enter a Group Name that is 100 or more characters long.
    13. Enter a valid Description, Min Goals, Max Goals, and Weightage (%).
    14. Click the "Create" button.
    15. Hover over the truncated name in the new row.
- Expected Results:
    - Either the input enforces a sensible character limit at entry time, or the saved name is truncated with an ellipsis in the row.
    - Hovering the truncated name reveals the full name in a tooltip if a tooltip is provided.
    - The row layout is not broken by the long name.
- Priority: Medium

### GG-138 — Setting Min Goals or Max Goals to a negative number is rejected
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
    11. Click the "+ Add Goal Group" button (or the pencil icon to edit an existing group).
    12. Enter a valid Group Name and Description.
    13. Set Min Goals to -1 (or set Max Goals to -1).
    14. Set Weightage (%) to a valid value below 100.
    15. Click the "Create" button (or "Update" on the edit form).
    16. Observe the response.
- Expected Results:
    - The submission is rejected with a clear validation message about the negative range.
    - No group is created or updated with negative goal values.
- Priority: High

### GG-139 — Setting both Min Goals and Max Goals to 0 is rejected
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
    11. Click the "+ Add Goal Group" button.
    12. Enter a valid Group Name and Description.
    13. Set Min Goals to 0 and Max Goals to 0.
    14. Set Weightage (%) to a valid value below 100.
    15. Click the "Create" button.
    16. Observe the response.
- Expected Results:
    - The submission is rejected with a clear validation message stating that the goal range must allow at least one goal.
    - No group is created with a 0-0 range.
- Priority: High

### GG-140 — Rapidly clicking the same template card creates the expected number of independent groups
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
    11. Click the Financial template card 5 times in rapid succession.
    12. Observe the group list and the Active count badge.
- Expected Results:
    - Exactly 5 new Financial template rows are added (no skipped clicks, no duplicates beyond the click count).
    - The Active count badge increments by exactly 5.
    - No frozen UI, broken state, or error appears.
- Priority: Medium

### GG-141 — Searching with regex-like special characters does not crash the page or return false matches
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
    11. Type ".*" into the search bar and observe the list.
    12. Clear the search bar and type "(business)".
    13. Clear the search bar and type "[Personal]".
    14. Clear the search bar and type "\d+".
- Expected Results:
    - For each special-character query the page does not crash and no console errors are produced.
    - The search treats the input as a literal substring (it does not match groups it should not match by interpreting the regex syntax).
    - When no group name contains the literal characters, the empty result message is shown.
- Priority: Medium

### GG-142 — A list with 50 or more goal groups remains scrollable and the search bar still filters in real time
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
    11. Confirm or set up a state with 50 or more goal groups in the list (test data prerequisite).
    12. Scroll the group list from top to bottom.
    13. Type a partial search term that matches a known group name.
- Expected Results:
    - The list scrolls smoothly without freezing or visible row overlap.
    - The search bar continues to filter in real time without noticeable lag.
    - Matching rows are shown and non-matching rows are hidden.
- Priority: Medium

### GG-143 — Browser refresh after applying templates but before clicking Save shows the unsaved-state behaviour
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
    11. Apply 2 or 3 templates from Quick Start Templates (do not click Save).
    12. Refresh the browser tab.
    13. Observe the group list after reload.
- Expected Results:
    - The page either restores the unsaved template additions, or returns to the prior saved state with a clear indication that unsaved changes were lost.
    - No partial or stale rows remain.
    - Behaviour is consistent with the application's documented save policy for the wizard.
- Priority: High

### GG-144 — Browser back button mid-edit returns to Step 4 without persisting unsaved edits
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
    11. Click the pencil icon on a group row and change the Group Name without saving.
    12. Click the browser back button.
    13. Use the breadcrumb or Continue button to return to the Goal Groups step.
    14. Observe the group row.
- Expected Results:
    - The back button returns the user to Step 4 (Proficiency Profile) or the prior screen.
    - When the user returns to Step 5, the group row still shows the original name; the unsaved edit is discarded.
    - No silent data persistence of the cancelled edit.
- Priority: Medium

### GG-145 — Setting Weightage above 100% on the Add Goal Group form is rejected
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
    11. Click the "+ Add Goal Group" button.
    12. Enter a valid Group Name (e.g., "Test"), Description, Min Goals, and Max Goals.
    13. Set Weightage (%) to 150.
    14. Click the "Create" button.
    15. Observe the response.
- Expected Results:
    - The submission is rejected with a validation message; the helper text below the Weightage field states "Each goal group's weightage must be less than 100%."
    - No group is added to the list with a weightage above 100%.
- Priority: High

---

## 13. Security

### GG-146 — Script payload entered as the Group Name is rendered as escaped text in the group row
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
    11. Click the "+ Add Goal Group" button.
    12. Enter `<script>alert('xss-name')</script>` into the Group Name field.
    13. Enter a benign Description and accept the default Min Goals, Max Goals, and Weightage (%).
    14. Click the "Create" button.
    15. Observe the new group row and the browser.
- Expected Results:
    - No JavaScript alert dialog appears.
    - The group row displays the payload as literal characters (the `<script>...</script>` text), not as executed markup.
    - The application either persists the row with the escaped string or rejects the input with a clear validation message; no script is parsed or executed.
    - No console errors related to script injection are produced.
- Priority: High

### GG-147 — Script payload entered as the Description is rendered as escaped text in the row and the Edit dialog
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
    11. Click the "+ Add Goal Group" button.
    12. Enter a benign Group Name (e.g., "XSS Test").
    13. Enter `<img src=x onerror=alert('xss-desc')>` into the Description field.
    14. Set valid Min Goals, Max Goals, and Weightage (%).
    15. Click the "Create" button.
    16. Observe the new group row.
    17. Click the pencil icon on the new row to open the Edit dialog and observe the Description field value.
- Expected Results:
    - No JavaScript alert dialog appears at any point.
    - The group row's description area shows the payload as literal text, not as a rendered image element.
    - The Edit dialog pre-fills the Description field with the literal payload string (not as parsed HTML).
    - No broken layout, console error, or network beacon is produced by the payload.
- Priority: High

### GG-148 — Searching for a script-like string in the search bar treats it as a literal substring and does not execute it
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
    11. Click the "Search goal groups..." search bar.
    12. Type `<script>alert(1)</script>`.
    13. Observe the page and the group list.
    14. Clear the search bar and type `"><img src=x onerror=alert(1)>`.
    15. Observe the page again.
- Expected Results:
    - No JavaScript alert dialog appears for either query.
    - The search treats both inputs as literal substrings; since no existing group name contains those characters, the empty result message is shown.
    - The search bar displays the typed characters verbatim and does not break the page layout.
    - No console errors are produced.
- Priority: High

### GG-149 — Step 5 (Goal Groups) is not reachable via the sidebar for non-HR-Admin roles
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
    - As Supervisor or Employee, the "Setup & Configuration" sidebar group is not displayed, so Definition Wizard (and therefore Step 5) is not reachable via the nav.
    - Direct navigation to `/setup/definitions` as Supervisor or Employee does not render the Goal Groups step; the user is redirected to a role-appropriate home or shown an access-denied state.
    - Restoring the role to HR Admin brings back the "Setup & Configuration" group with Definition Wizard visible.
- Priority: High

### GG-150 — Signing out while the Create or Edit Goal Group dialog is open discards the in-progress edit
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
    11. Click the pencil icon on an existing group row to open the Edit Goal Group dialog.
    12. Change the Group Name to a new value, but do not click "Update".
    13. Open the sidebar user menu and click "Sign Out".
    14. After being returned to the sign-in screen, sign back in as the same HR Admin user.
    15. Navigate back to Step 5 of the Definition Wizard.
    16. Observe the group row that was being edited.
- Expected Results:
    - Signing out returns the user to the sign-in screen and closes the dialog without persisting the in-progress edit.
    - After signing back in, the group row shows the original (pre-edit) values; the unsaved name change is not present.
    - No half-saved Group Name appears in the row, the search bar suggestions, or the Active count badge.
- Priority: High

### GG-151 — Inactivity past the session timeout while the Edit Goal Group dialog is open requires re-authentication before Update succeeds
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
    11. Click the pencil icon on an existing group row to open the Edit Goal Group dialog.
    12. Change the Group Name and Description to new valid values, but do not click "Update".
    13. Leave the tab idle past the application's inactivity timeout.
    14. Click the "Update" button.
- Expected Results:
    - The Update attempt prompts the user to re-authenticate (redirect to sign-in, inline session-expired modal, or toast directing the user to sign in again).
    - No silent update occurs against an expired session; the group row continues to show its pre-edit values until a successful re-auth.
    - After re-authenticating, the user is returned to Step 5 with the dialog state preserved or explicitly discarded, but not silently applied.
- Priority: Medium

### GG-152 — Editing the same group in two browser tabs and updating each handles the concurrent change gracefully
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
    11. Open the Definition Wizard in a second browser tab so both Tab A and Tab B are on Step 5 and both show the same group row (e.g., "Business Objectives").
    12. In Tab A, open the Edit Goal Group dialog for that row, change the Description, and click "Update".
    13. In Tab B (which still has the stale state), open the Edit Goal Group dialog for the same row, change the Weightage (%), and click "Update".
    14. Reload Tab A and observe the row.
- Expected Results:
    - Both tabs allow editing without crashing or freezing.
    - On Update from Tab B the application behaves consistently — either last-write-wins (Tab B's weightage persists, no error), a conflict warning, or an HTTP 400/409 with a toast.
    - Reloading Tab A shows the final persisted state, not a mixture of in-memory edits from each tab.
- Priority: Medium

### GG-153 (New) — Continue is blocked when combined weightages of all goal groups exceed 100%
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
    11. Ensure the existing pre-populated groups are visible (Business Objectives: 50%, Personal Development: 30%, Team Collaboration: 25% = 105% total).
    12. Confirm the combined weightage of all active groups exceeds 100%.
    13. Click the "Continue" button.
    14. Observe the response.
- Expected Results:
    - The Continue action is blocked.
    - An HTTP 400 error is returned.
    - A toast message appears indicating that the total combined weightage of all goal groups must not exceed 100%.
    - The user remains on the Goal Groups page.
    - The step is not marked complete.
    - The goal groups remain displayed so the user can adjust individual weightages.
- Priority: High

### GG-154 (New) — Save & Exit persists data when navigating out and back to the Performance Management module
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
    11. Apply one or more Quick Start Templates or add a new goal group.
    12. Click the "Save & Exit" button in the footer.
    13. Observe the page where the user is taken (should be outside the wizard).
    14. Navigate to a different module or section of the application.
    15. Navigate back to the Performance Management module and re-enter the Definition Wizard.
    16. Advance to Step 5 (Goal Groups) by clicking Continue through Steps 1–4.
    17. Observe the goal groups list.
- Expected Results:
    - After clicking Save & Exit, the user exits the wizard and is taken to the dashboard or relevant landing page.
    - After navigating away and returning to the Performance Management module, the Definition Wizard retains the saved Step 5 state (the goal groups and settings from step 11 are still present).
    - No data loss occurs between the exit and re-entry.
    - The step progress is retained and the breadcrumb reflects the completed steps.
- Priority: High

