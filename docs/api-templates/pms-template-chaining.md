# PMS API templates - how they chain

Reference for the `pms-cycle-*` (Performance Cycle) and `pms-assessment-*`
(My Assessments / Team Assessments) API templates: the order they run in,
the ids each hands to the next, and the server rules that make a run fail.
The design of the feature itself is in
`docs/superpowers/specs/2026-09-28-api-templates-design.md`.

Everything here was proven on **hosted dev01** (`hrmmainphdev01.phrsandbox.dev`)
on 2026-09-29.

## Environment

- **Database:** hosted dev01 runs on `hrmmain_philippines` (server
  `sgdev01db02`). Read it with the app's `db_query` tool, or `pms-sql
  -Target ro`. `pms-sql`'s default `dev` target is `hrmmain_philippinesdev`,
  a different database.
- **Accounts:** a template runs as an Auto Run account key. `conrad` (Conrad
  M Potter, 00000147) builds cycles and is the demo appraisee; `imly` (Imly
  Taylor, 000049) and `emma` (Emma Phillips, 000001) are managers/reviewers.
- **One session per account.** PeoplesHR logs an account out wherever else
  it is signed in when it signs in again ("Continue here"). Keep the
  account signed out of browsers while templates run, or saves come back
  as empty 400s.
- **Empty 400s.** A 400 with no body means the request was refused before
  any handler read it (busy shared server, or a taken-over session). The
  runner retries such a step up to three times (1, 3 and 5 seconds apart),
  each with a fresh token; a fourth one fails it. On 2026-09-29 about two
  in five writes on hosted came back this way, independently of the try
  before.
- **Letter case.** Hosted PMSV10 keeps its anti-forgery cookie on the
  lowercase path `/hr/pmsv10`. Write paths as the UI calls them (lowercase);
  the runner adapts a path whose case differs only from a cookie's.
- **Token pages:** `/hr/pmsv10/performancecycle?mode=create` for the cycle
  templates (needs cycle-admin rights), `/hr/pmsv10/updatehub` for the
  assessment templates (any employee or manager can open it; it only reads).
- **Names:** anything created must read like real demo data ("FY2026 Annual
  Performance Review"), never "test".

## Performance Cycle (`pms-cycle-*`, 36 templates)

Run in this order on one cycle. Each takes `cycleId` except the first.

| Step | Template(s) | Notes |
|---|---|---|
| 1 Cycle setup | `save-progress` (creates, outputs `cycleId`), `save`, `copy-from-previous` | `performanceMethod` "90"/"180"/"360" with `sourceTemplateId` 1/2/3. Copy-from-previous needs a DRAFT target. |
| 2 Evaluation rules | `save-eval-rules`, `save-eval-rules-progress` | Goal groups need goals on + custom goal type; the competencies step needs competencies on. Rating method ids differ per environment (hosted: 32-34, 88-104). |
| 3 Timeline | `regenerate-stages`, `auto-generate-dates`, `auto-balance-weights`, `apply-marks-allocation`, `save-timeline`, `save-timeline-progress`, `split-annual-cycle`, `unsplit-annual-cycle` | The saves take `annualGoalAllocation` + `annualCompetencyAllocation`, which must total 100 when both are enabled (e.g. 60/40). Covers goals on, no intermediate stages, no calibration. Split breaks a later Save & Continue on the same cycle - use it on a throwaway cycle. |
| 4 Evaluators | `save-evaluators`, `save-evaluators-progress` | Roles self/manager/reviewer; enabled weights total 100 (e.g. 20/50/30). 180/360 need self on; 90 needs it off. Toggling the reviewer role resets Participants. |
| 5 Competencies | `assign-competencies` (outputs `assignedProfileIds`), `save-weights` (`profileId` from assign), `continue-competencies`, `delete-profile-assignment`, `copy-from-wizard` | One designation per run; the first area and its first two competencies. Area weights total 100 per profile, competency weights 100 per area. `copy-from-wizard` re-seeds everything - never on a cycle you have weighted. |
| 6 Participants | `save-participants`, `save-participants-progress`, `remove-inactive-participants` | `participants` is a list: `{empNumber, isActive, managerEmpNumber, managerAssignmentType, reviewer1EmpNumber, reviewerAssignmentType, sortOrder}`; assignment type `immediate_manager` or `individually_assigned`. Replaces the whole list. |
| 7 Goal groups | `goal-groups-save`, `-continue`, `-add-group`, `-rename-group`, `-delete-group`, `-assign-personalization`, `-delete-personalization`, `-clear-personalizations` | Save first (it turns the preview groups' negative ids into real ones). Continue needs group weights totalling 100 and a rating method on each enabled group; it is tied to hosted's two wizard groups. |
| 8 Configurations | `cycle-configurations-save`, `-continue` | 12 keys, '0'/'1'. `allowEvaluationOverlap` must be "1" if a participant is already in another active cycle. |
| 9 Publish | `publish`, then `manage-toggle-publish`, `manage-delete` | Publish needs Draft + every step complete. Toggle refuses drafts; Delete accepts drafts only. |

## My Assessments (`pms-assessment-*`)

Needs a published cycle where the employee is a participant with a manager
(and reviewer), and competencies assigned to their designation. Stage dates
are not enforced by the server. Ids come from these read-only queries
(replace the cycle, stage and employee):

```sql
-- goal ids at a stage (goal planning, or the annual copy)
SELECT g.participant_goal_id, g.goal_title, g.cycle_goal_group_id
FROM PeoplesHR.perf_cp_goal g JOIN PeoplesHR.perf_cp_goal_plan p ON p.goal_plan_id = g.goal_plan_id
WHERE p.performance_cycle_id = 280 AND p.timeline_stage_id = 3606 AND p.participant_emp_number = '00000147';

-- competency ids (after the annual Competencies step was opened)
SELECT c.participant_comp_id, c.competency_name, c.rating_method_id
FROM PeoplesHR.perf_cp_comp c
JOIN PeoplesHR.perf_cp_comp_area a ON a.participant_comp_area_id = c.participant_comp_area_id
JOIN PeoplesHR.perf_cp_comp_group g ON g.participant_comp_group_id = a.participant_comp_group_id
JOIN PeoplesHR.perf_cp_comp_plan p ON p.participant_comp_plan_id = g.participant_comp_plan_id
WHERE p.performance_cycle_id = 280 AND p.timeline_stage_id = 3606 AND p.emp_number = '00000147';

-- status trail
SELECT stage_status_id, stage_key, stage_role, stage_status, actioned_by
FROM PeoplesHR.perf_cp_stage_status
WHERE performance_cycle_id = 280 AND emp_number = '00000147' ORDER BY stage_status_id;
```

**Goal plan (goal-planning stage), with a rejection:**

1. `save-goal-plan` (employee) - at least one goal per enabled group, weights 100 per group.
2. `submit-goal-plan` (employee) -> manager `in_review`.
3. `manager-reject-goal-plan` (manager, `rejectComment`) -> manager `rejected`.
4. `submit-goal-plan` again with the corrected goal in `upserts` (same `participantGoalId`s - never re-send new goals, they duplicate).
5. `manager-approve-goal-plan` (manager).
6. `acknowledge-goal-plan` (employee) - acknowledge only. The annual goal and
   competency ids appear when the rating templates first open the annual
   stage's steps.
7. Optional, between 6 and the first rating: `revise-goal-plan` (employee,
   `stageId` = the goal-planning stage). Needs `goal_revision_enabled = 1`
   (else 179000146) and is refused once the annual copy of the plan exists
   (179000147). Send each changed goal in `upserts` with its
   `participantGoalId` (a `null` id adds a goal, every time it is sent); each
   group must still meet its minimum count and total 100.

**Self-assessment (annual stage), with a rejection:**

1. `save-goal-rating` per goal (`ratingGradeId`: hosted rating method 33 = 434 A ... 437 D).
2. `save-competency-rating` per competency (`ratingMark` 1-5, comment param `ratingComment`).
3. `save-fdp` (achievements, development areas, job preferences).
4. `submit-assessment` (employee) -> manager `in_review`. Irreversible for the employee; never re-send it on a submitted assessment.
5. `manager-flag-revision` (manager, `stepKey`, comment); `manager-clear-flag-revision` to unflag; `manager-fdp-flag-revision` / `manager-clear-fdp-flag-revision` for the FDP step.
6. `manager-reject-goals` (manager) -> employee `not_started` again.
7. The employee re-rates what was flagged and runs `submit-assessment` again.

`revise-goal-plan` was proven on cycle 281 "H2 2026 Performance Review"
(goal revision on). Not proven: the three attachment deletes (need a file
uploaded through the UI first - uploads cannot be templated).

## Team Assessments (manager and reviewer)

Manager and reviewer use the same `pms-assessment-manager-*` templates. The
server takes the role from the participant row (manager, else reviewer);
`actingRole` ("manager" / "reviewer") only settles it for someone who is
both. Manager and reviewer rate the same goal and competency ids as the
employee. Each writes only while their own latest status is `in_review`.

**From the employee's submission to a finished stage, with a reviewer rejection:**

1. Manager: `manager-save-goal-rating` per goal, `manager-save-competency-rating`
   per competency, optionally `manager-save-fdp` (comments go on the
   employee's entry ids).
2. Manager: `manager-approve-goals` -> manager `submitted`, reviewer
   `in_review`. Approval needs the approver's mark on every goal and
   competency (179000019 otherwise); comments are optional.
3. Reviewer: the same rating templates with `actingRole` "reviewer".
4. Reviewer send-back: `manager-flag-revision` then `manager-reject-goals`,
   both with `actingRole` "reviewer" -> reviewer `rejected`, manager
   `in_review`. It goes back to the manager, not the employee; every rating
   stays.
5. Manager re-rates what was flagged and runs `manager-approve-goals` again,
   which archives the reviewer's flag (`manager_approve`) -> reviewer
   `in_review`.
6. Reviewer: optionally `manager-save-fdp`, then `manager-approve-goals` with
   `actingRole` "reviewer". This is the final step (there is no manual
   final-rating handler): it writes the stage completion row, the
   `perf_cp_stage_score` row and `perf_cp_overall_rating`.

Proven on cycle 280 as `imly` (manager) and `emma` (reviewer): final score
0.8448 = goals 0.9413 x 60% + competencies 0.70 x 40%.

**Manager changes to a submitted goal plan** (manager goal_planning status
`in_review`; proven on cycle 281 as `conrad` on Quinton's plan, account
`quinton`):

1. `manager-save-goal-plan` - an upsert with a `participantGoalId` edits
   that goal (recorded as a manager `edit` for the employee to acknowledge;
   keep each group at 100); a `null` id adds a goal every run. `deletes` is a
   soft delete (a pending manager `delete` in `perf_cp_goal_modification`).
2. `manager-restore-goal` - removes a pending manager delete (179000148 if
   there is none).
3. `manager-approve-goal-plan` -> employee `pending_ack`.

Not proven: the three manager attachment deletes (need a UI upload first).

## Known gaps

- Several templates build fixed shapes (one designation, first area, two
  competencies; two goal groups; one timeline layout) because a template
  cannot build a list of objects from captures.
- PMS server issues noticed while mapping (not fixed): `SubmitAssessment`
  has no turn check (re-sending puts the assessment back in the manager's
  queue); `SaveGoalRating` does not check the goal belongs to the caller;
  `SaveGoalPlan` can change a plan the manager is reviewing;
  `SaveGoalRatingAsManager` does not check the goal belongs to the
  appraisee; `SaveFdpAsManager` answers an unknown entry id with a 500
  instead of a 422.
