# Proficiency Profile — UI Manual Test Cases

These test cases are based on direct UI observation of the Proficiency Profile screen (Step 4 of 6) in the Definition Wizard. Database, API, and implementation details are intentionally omitted.

Assumptions used for this test set:
- The user is logged in as an HR Admin (Sarah Thompson role observed in the UI).
- The wizard setup includes Competencies as a selected evaluation component.
- "Position" is the default active grouping type on page load (Recommended).

## Topics Covered

- Page load and layout display
- Step progress indicator and breadcrumb navigation
- Industry recommendation badge
- Grouping type card selection and switching
- Recommended card visual state
- Select Positions panel and dropdown
- Select All and Clear All button behaviour
- Empty state display
- Position accordion panel behaviour after selection
- Footer button behaviour (Previous, Continue, Save, Save & Exit)
- Header button behaviour (Cancel, Save)
- Sidebar navigation and collapse
- Keyboard navigation and focus order
- Hover and visual feedback on grouping cards
- Truncation and tooltip behaviour for long values
- Bulk selection and large-list scrolling
- Browser refresh and back-button behaviour
- Switching grouping type after assignments
- Role gating for the Definition Wizard sidebar entry and the /setup/definitions URL
- Sign-out, session timeout, and concurrent-tab handling on Step 4

---

## 1. Page Load and Layout

### PP-101 — Proficiency Profile page opens at Step 4 of 6 with the correct layout
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
- Expected Results:
    - The page heading reads "Definition Wizard" with subtitle "Configure your performance management system".
    - The step counter shows "Step 4 of 6".
    - The progress bar shows 67% Complete.
    - The "Proficiency Profile" section heading is visible in the main content area.
    - The footer contains Previous, Continue, Save, and Save & Exit buttons.
    - The header contains Cancel and Save buttons.
    - No broken layout or missing content is visible.
- Priority: High

### PP-102 — Step breadcrumb shows completed steps with green checkmarks
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
    9. Observe the step breadcrumb bar below the progress bar.
- Expected Results:
    - Rating Methods, Proficiency Levels, and Competencies each show a filled green circle with a checkmark.
    - Step 4 (Proficiency Profile) is highlighted as the current active step.
    - Step 5 (Goal Groups) is visible but not yet accessible or shown as incomplete.
    - Navigation arrows are present to scroll the breadcrumb if more steps exist.
- Priority: High

### PP-103 — "Templates recommended for your industry" badge is displayed
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
    9. Observe the area near the Proficiency Profile section heading.
- Expected Results:
    - A badge or pill labelled "Templates recommended for your industry" is visible alongside the heading.
    - The badge includes a decorative icon (spark or similar).
    - The badge is informational only and not interactive.
- Priority: Medium

---

## 2. Grouping Type Card Display

### PP-104 — All 4 grouping type cards are displayed with correct labels and descriptions
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
    9. Observe the "Group employees by:" section.
- Expected Results:
    - The section heading reads "Group employees by:".
    - The instruction text reads "Select how you want to categorize employees for competency profile assignment."
    - Four cards are displayed in a row: Designation, Salary Grade, Corporate Title, and Position.
    - Each card shows its name and a one-line description:
        - Designation: "Group by job designation or role title"
        - Salary Grade: "Group by salary band or pay grade"
        - Corporate Title: "Group by corporate hierarchy title"
        - Position: "Group by specific job position"
- Priority: High

### PP-105 — Position card has the Recommended badge and is selected by default
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
    9. Observe the four grouping type cards without taking any action.
- Expected Results:
    - The Position card has a green "Recommended" badge displayed in its top-right corner.
    - The Position card has a green border and a checkmark, indicating it is the selected grouping type.
    - The other three cards (Designation, Salary Grade, Corporate Title) have a neutral grey border and no checkmark.
- Priority: High

### PP-106 — Selecting a different grouping type activates it and deselects Position
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
    9. Click the Designation card.
    10. Observe the card states.
- Expected Results:
    - The Designation card becomes active (green border and checkmark).
    - The Position card loses its active state (reverts to grey border, no checkmark).
    - The values panel below updates to show Designation-specific values.
    - Only one card is in the active state at a time.
- Priority: High

### PP-107 — Selecting Salary Grade activates it and updates the values panel
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
    9. Click the Salary Grade card.
    10. Observe the card state and the values panel.
- Expected Results:
    - The Salary Grade card becomes active (green border and checkmark).
    - All other cards return to a neutral state.
    - The values panel heading updates to reflect the Salary Grade grouping type.
- Priority: High

### PP-108 — Selecting Corporate Title activates it and updates the values panel
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
    9. Click the Corporate Title card.
    10. Observe the card state and the values panel.
- Expected Results:
    - The Corporate Title card becomes active (green border and checkmark).
    - All other cards return to a neutral state.
    - The values panel heading updates to reflect the Corporate Title grouping type.
- Priority: High

### PP-109 — Re-selecting Position after switching to another type restores the Position panel
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
    9. Click the Designation card to switch away from Position.
    10. Click the Position card.
    11. Observe the card state and the values panel.
- Expected Results:
    - The Position card becomes active (green border, checkmark, Recommended badge).
    - Designation returns to a neutral state.
    - The "Select Positions" panel is shown again.
- Priority: Medium

---

## 3. Select Positions Panel

### PP-110 — Select Positions panel is visible when Position grouping type is active
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
    9. Observe the content area below the grouping type cards.
- Expected Results:
    - A panel titled "Select Positions" is displayed.
    - The instruction text reads "Choose one or more values to assign competency profiles."
    - A "Select All" button and a "Clear All" button are visible on the right side of the panel header.
    - A combobox with placeholder text "Select positions..." is shown.
- Priority: High

### PP-111 — Clicking the positions combobox opens a dropdown with available positions
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
    9. Click the "Select positions..." combobox.
    10. Observe the dropdown.
- Expected Results:
    - A dropdown opens listing available position values from the HR system.
    - The list is scrollable if it contains many entries.
    - Individual items can be clicked to select them.
- Priority: High

### PP-112 — Selecting one or more positions from the dropdown adds them to the panel
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
    9. Click the "Select positions..." combobox to open the dropdown.
    10. Click one or more position items from the dropdown.
    11. Close the dropdown.
    12. Observe the panel below.
- Expected Results:
    - The selected positions appear as accordion panels or group entries below the combobox.
    - The empty state ("No groups selected") is no longer displayed.
    - Each selected position has its own panel or row.
- Priority: High

### PP-113 — Select All button selects every available position at once
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
    9. Click the "Select All" button.
    10. Observe the combobox and the panel below.
- Expected Results:
    - All available positions are selected simultaneously.
    - The combobox reflects that all positions are selected.
    - Accordion panels or group rows appear for every position.
    - The empty state is replaced by the group panels.
- Priority: High

### PP-114 — Clear All button deselects all positions and restores the empty state
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
    9. Click the "Select positions..." combobox and select one or more positions, then close the dropdown.
    10. Click the "Clear All" button.
    11. Observe the combobox and the panel below.
- Expected Results:
    - All selected positions are removed.
    - The combobox returns to showing "Select positions..." placeholder.
    - The empty state icon and message ("No groups selected") reappear.
- Priority: High

---

## 4. Empty State

### PP-115 — Empty state is shown when no positions are selected
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
    9. Ensure the positions combobox shows "Select positions..." with no selections.
    10. Observe the area below the combobox.
- Expected Results:
    - A group icon (people silhouette) is displayed.
    - The primary message reads "No groups selected".
    - The secondary message reads "Select one or more positions above to assign competency profiles."
    - No accordion panels or group rows are shown.
- Priority: High

### PP-116 — Empty state disappears once at least one position is selected
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
    9. Confirm the empty state is visible (combobox empty, "No groups selected" message displayed).
    10. Click the combobox and select any position.
    11. Observe the panel area.
- Expected Results:
    - The empty state (icon and both text lines) is no longer visible.
    - A panel or group row for the selected position appears in its place.
- Priority: High

---

## 5. Footer Button Behaviour

### PP-117 — Previous button navigates to Step 3 (Competencies)
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
    9. Click the "← Previous" button in the footer.
- Expected Results:
    - The user is taken back to the Competencies step (Step 3).
    - The step counter updates to reflect Step 3.
    - No data entered on the Proficiency Profile page is lost if it was saved.
- Priority: High

### PP-118 — Continue button is visible and styled as the primary action
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
    9. Observe the footer button row.
- Expected Results:
    - The Continue button is displayed with a green background, making it the visually primary action.
    - The button shows a right-arrow icon and is labelled "Continue".
- Priority: Medium

### PP-119 — Clicking Continue without selecting positions shows validation or advances appropriately
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
    9. Ensure the combobox shows "Select positions..." with no selections.
    10. Click the "Continue" button.
    11. Observe the result.
- Expected Results:
    - Either a validation message is displayed instructing the user to select at least one position, or
    - If positions are not mandatory for progression, the user advances to Step 5 (Goal Groups).
    - No unintended navigation or silent failure occurs.
- Priority: High

### PP-120 — Clicking Continue after selecting positions and making assignments advances to Step 5
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
    9. Select one or more positions from the combobox.
    10. Open an accordion panel and assign competency levels.
    11. Click the "Continue" button.
- Expected Results:
    - The user is taken to Step 5 (Goal Groups).
    - The step breadcrumb updates to show Proficiency Profile as complete (green checkmark).
    - The step counter updates to "Step 5 of 6".
- Priority: High

### PP-121 — Save button saves the current state without navigating away
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
    9. Select one or more positions and make competency assignments.
    10. Click the "Save" button in the footer (floppy disk icon).
    11. Observe the page.
- Expected Results:
    - The current state is saved.
    - The user remains on the Proficiency Profile page.
    - A confirmation indicator (toast or visual feedback) is shown.
    - No navigation occurs.
- Priority: High

### PP-122 — Save & Exit saves the current state and returns the user out of the wizard
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
    9. Make some progress on the page (e.g., select one or more positions).
    10. Click the "Save & Exit" button in the footer.
    11. Observe where the user is taken.
- Expected Results:
    - The current state is saved.
    - The user exits the wizard and is taken to the relevant landing page or dashboard.
    - The wizard step progress is retained for the next session.
- Priority: High

---

## 6. Header Button Behaviour

### PP-123 — Cancel button in the header exits without saving unsaved changes
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
    9. Select one or more positions (do not click Save).
    10. Click the "Cancel" button in the header area.
    11. Observe the result.
- Expected Results:
    - The user is taken away from the Proficiency Profile step.
    - Unsaved changes are either discarded with a confirmation prompt, or
    - A dialog asks the user to confirm they want to leave without saving.
    - No unintended data persistence from the cancelled session.
- Priority: High

### PP-124 — Save button in the header saves the current state without navigating away
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
    9. Make changes on the page.
    10. Click the "Save" button in the page header (distinct from the footer Save button).
    11. Observe the page.
- Expected Results:
    - The current state is saved.
    - The user remains on the Proficiency Profile page.
    - A confirmation indicator is shown.
- Priority: Medium

---

## 7. Step Breadcrumb Navigation

### PP-125 — Clicking a completed step in the breadcrumb navigates to that step
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
    9. Click the "Rating Methods" breadcrumb button.
    10. Observe navigation.
- Expected Results:
    - The user is taken to the Rating Methods step.
    - The step counter and content area update accordingly.
- Priority: Medium

### PP-126 — Clicking a future locked step in the breadcrumb is blocked
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
    9. Attempt to click the Step 5 breadcrumb button.
    10. Observe the result.
- Expected Results:
    - The user is not navigated to Step 5.
    - The button appears disabled or shows a tooltip indicating that the previous step must be completed first.
- Priority: High

---

## 8. Sidebar Navigation

### PP-127 — Left sidebar is visible and shows the current navigation context
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
    9. Observe the left sidebar.
- Expected Results:
    - The PeoplesHR brand and logo are visible at the top of the sidebar.
    - The logged-in user (Sarah Thompson, HR Admin) is shown.
    - "Setup & Configuration" is expanded, showing Definition Wizard and Define Company Objectives.
    - "Definition Wizard" is highlighted as the current location.
    - Performance Cycles and Administration sections are collapsed.
- Priority: Medium

### PP-128 — Collapse button hides the sidebar and expands the main content area
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
    9. Click the "Collapse" button at the bottom of the sidebar.
    10. Observe the layout.
- Expected Results:
    - The sidebar collapses and is no longer fully visible.
    - The main content area expands to fill the additional space.
    - An expand or arrow button is available to restore the sidebar.
- Priority: Low

---

## 9. Negative and Edge Cases

### PP-129 — Tab order through grouping cards, combobox, and footer is keyboard-traversable
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
    9. Press Tab repeatedly from the page top until focus reaches the first interactive control on the Proficiency Profile page.
    10. Continue pressing Tab to move focus through the Designation, Salary Grade, Corporate Title, and Position cards.
    11. Press Enter or Space on a focused card.
    12. Continue Tab to reach the positions combobox, then the Continue, Save, and Save & Exit buttons.
- Expected Results:
    - A visible focus indicator appears on each interactive element.
    - Tab order matches the visual reading order (left to right, top to bottom).
    - Pressing Enter or Space on a focused grouping card activates that grouping type.
    - All footer buttons are reachable using the keyboard alone.
- Priority: Medium

### PP-130 — Hovering a grouping card shows visual feedback without changing the active selection
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
    9. Move the cursor over the Designation card without clicking.
    10. Move the cursor over the Salary Grade and Corporate Title cards in turn.
    11. Observe the active selection indicator.
- Expected Results:
    - Each hovered card shows a visual change (e.g., pointer cursor, border highlight, or subtle shadow).
    - The currently selected card (Position by default) remains the active selection while hovering other cards.
    - Hover does not commit any change.
- Priority: Low

### PP-131 — A very long position name is truncated with a tooltip in the dropdown and the panel
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
    9. Open the positions combobox.
    10. Locate or set up (via test data) a position whose name is 80 or more characters long.
    11. Select that position and close the dropdown.
    12. Hover over the truncated name in the dropdown row and in the accordion panel header.
- Expected Results:
    - The long name is truncated with an ellipsis (or wrapped) and does not overflow the dropdown or panel bounds.
    - Hovering the truncated text reveals the full name in a tooltip if a tooltip is provided.
    - The page layout is not broken by the long name.
- Priority: Medium

### PP-132 — Selecting all positions when 50 or more positions exist keeps the panel scrollable and responsive
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
    9. Confirm the positions list contains 50 or more available positions (test data prerequisite).
    10. Click the "Select All" button.
    11. Scroll the panel below the combobox.
- Expected Results:
    - All positions are selected and rendered as accordion panels.
    - The panel area remains scrollable with no overlapping or clipped rows.
    - Scrolling is smooth and does not freeze the page.
    - The Continue button remains responsive.
- Priority: Medium

### PP-133 — Refreshing the browser after selecting positions but before clicking Save shows the unsaved-state behaviour
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
    9. Select one or more positions from the combobox (do not click Save).
    10. Refresh the browser tab.
    11. Observe the page state after reload.
- Expected Results:
    - The page either restores the unsaved selections, or returns to the empty state with a clear indication that unsaved changes were lost.
    - No partial or stale data is rendered.
    - The behaviour is consistent with the application's documented save policy for the wizard.
- Priority: High

### PP-134 — Switching grouping type after making competency assignments handles existing assignments cleanly
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
    9. Confirm Position is the active grouping type.
    10. Select one or more positions and assign competency levels in the accordion panel.
    11. Click the Designation card to switch the grouping type.
    12. Click the Position card again to return.
- Expected Results:
    - Switching grouping types does not crash the page or break the layout.
    - The application either warns the user that switching will discard the assignments, retains the assignments when Position is re-selected, or clearly informs the user that the assignments have been cleared.
    - The values panel updates correctly for the newly selected grouping type each time.
- Priority: High

### PP-135 — Browser back button after saving Step 4 returns to Step 3 without losing the saved data
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
    9. Select positions, make competency assignments, and click "Save" in the footer.
    10. Click the browser back button.
    11. Observe the destination.
    12. Use the breadcrumb or Continue button to return to Step 4 and observe the saved state.
- Expected Results:
    - The browser back button returns the user to Step 3 (Competencies) or the prior screen, consistent with the application's expected behaviour.
    - When the user returns to Step 4, the previously saved positions and assignments are still present.
- Priority: Medium

---

## 10. Security

### PP-136 — Definition Wizard sidebar entry is hidden for non-HR-Admin roles
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. Open the sidebar user menu by clicking the "Sarah Thompson HR Admin" button.
    2. From the "Switch Role" submenu, click "Supervisor".
    3. Observe the sidebar navigation.
    4. Re-open the sidebar user menu and switch back to "HR Admin".
    5. Re-open the sidebar user menu and click "Employee".
    6. Observe the sidebar navigation.
    7. Switch the role back to "HR Admin".
- Expected Results:
    - As Supervisor, the sidebar shows only "My Updates Hub" and "Performance Cycles"; the "Setup & Configuration" group (which contains Definition Wizard) is not displayed.
    - As Employee, the sidebar also does not show the "Setup & Configuration" group; Definition Wizard is not reachable via the navigation.
    - Switching back to "HR Admin" restores the full sidebar with Setup & Configuration → Definition Wizard visible.
- Priority: High

### PP-137 — Navigating directly to /setup/definitions as a non-HR-Admin role does not expose Step 4
- Preconditions: User is logged in as an HR Admin and clicked Definition Wizard.
- Steps:
    1. Open the sidebar user menu and switch the role to "Supervisor".
    2. Change the browser URL to `/setup/definitions` and press Enter.
    3. Observe the page that loads.
    4. Switch the role to "Employee" via the sidebar user menu.
    5. Change the browser URL to `/setup/definitions` and press Enter again.
    6. Observe the page that loads.
    7. Switch the role back to "HR Admin".
- Expected Results:
    - As Supervisor, the URL does not render the Definition Wizard; the user is redirected to the Supervisor home (Team Performance Dashboard) or shown an access-denied message.
    - As Employee, the same blocking behaviour is observed.
    - At no point are the Proficiency Profile grouping cards, the Select Positions panel, or any wizard footer buttons rendered for a non-Admin role.
- Priority: High

### PP-138 — Signing out while Step 4 has unsaved selections discards those changes
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
    9. Select one or more positions in the "Select positions..." combobox without clicking Save.
    10. Open the sidebar user menu and click "Sign Out".
    11. Sign back in as the same HR Admin user.
    12. Re-open the Definition Wizard and navigate to Step 4.
- Expected Results:
    - Signing out returns the user to the sign-in screen without auto-saving the unsaved Step 4 selections.
    - After signing back in, the Proficiency Profile page shows the last persisted state; the unsaved selections from before Sign Out are gone.
    - No partially-saved selections appear in the panel or accordion.
- Priority: High

### PP-139 — Inactivity past the session timeout while on Step 4 requires re-authentication before Save succeeds
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
    9. Select one or more positions on Step 4.
    10. Leave the browser tab idle past the application's inactivity timeout (do not interact with the page or keep the tab focused).
    11. Click the "Save" button in the footer.
- Expected Results:
    - The save attempt prompts the user to re-authenticate (a redirect to sign-in, an inline session-expired modal, or a toast directing the user to sign in again).
    - No silent save occurs against an expired session.
    - After re-authenticating, the user is returned to Step 4 with a clear indication of which selections (if any) were preserved.
- Priority: Medium

### PP-140 — Opening Step 4 in two browser tabs and saving from each handles the concurrent state gracefully
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
    9. Open the same Definition Wizard URL in a second browser tab so both tabs are on Step 4.
    10. In Tab A, select one position and click "Save".
    11. In Tab B (which still holds the pre-save state), select a different position and click "Save".
    12. Reload Tab A and observe the final state.
- Expected Results:
    - Both tabs allow editing without crashing or freezing.
    - On Save from Tab B the application behaves consistently — either last-write-wins (Tab B's selection persists, no error), a conflict warning, or an HTTP 400/409 with a toast.
    - Reloading Tab A shows the final persisted state, not a mixture of both tabs' in-memory states.
- Priority: Medium

### PP-141 (New) — Save & Exit persists data when navigating out and back to the Performance Management module
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
    9. Select one or more positions and make competency assignments.
    10. Click the "Save & Exit" button in the footer.
    11. Observe the page where the user is taken (should be outside the wizard).
    12. Navigate to a different module or section of the application.
    13. Navigate back to the Performance Management module and re-enter the Definition Wizard.
    14. Advance to Step 4 (Proficiency Profile) by clicking Continue through Steps 1–3.
    15. Observe the positions and assignments.
- Expected Results:
    - After clicking Save & Exit, the user exits the wizard and is taken to the dashboard or relevant landing page.
    - After navigating away and returning to the Performance Management module, the Definition Wizard retains the saved Step 4 state (the selected positions and competency assignments from step 9 are still present).
    - No data loss occurs between the exit and re-entry.
    - The step progress is retained and the breadcrumb reflects the completed steps.
- Priority: High

