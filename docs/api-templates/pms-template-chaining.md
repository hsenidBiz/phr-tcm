# PMS API templates - what the flows do not say

The ORDER the `pms-cycle-*` and `pms-assessment-*` templates run in, and
whether a record is ready for the next one, lives in two saved flows - ask
the app, not this page:

- `pms-performance-cycle` - subject `cycleId`. Setup, evaluation rules,
  timeline, evaluators, competencies, participants, goal groups,
  configurations, publish.
- `pms-assessment` - subject `participantId`
  (`perf_cycle_participant.cycle_participant_id`). Goal plan saved,
  submitted, approved, acknowledged (optionally revised), self-assessment
  submitted, manager approved, review complete.

Call `get_api_flow_progress` before every run of a template on a flow and
run the stage marked `next`; `list_api_templates` shows which template
performs each stage. This page keeps what a flow cannot express: the
environment, the values each template needs, the send-back round-trips and
the gaps. The feature's design is in
`docs/superpowers/specs/2026-09-28-api-templates-design.md`.

## Environment (hosted dev01)

- **Database:** `hrmmain_philippines` on `sgdev01db02`. Read it with the
  app's `db_query` tool, or `pms-sql -Target ro` - `pms-sql`'s default `dev`
  target is `hrmmain_philippinesdev`, a different database.
- **Accounts:** `conrad` (Conrad M Potter, 00000147) builds cycles and is the
  demo appraisee; `imly` (Imly Taylor, 000049) and `emma` (Emma Phillips,
  000001) manage and review; `quinton` (00000143) is an appraisee Conrad
  manages.
- **One session per account.** Signing in again ("Continue here") logs the
  account out everywhere else. Keep it signed out of browsers while
  templates run.
- **Empty 400s.** Refused before any handler read it; the runner tries the
  step up to three more times with a fresh token. On 2026-09-29 about two in
  five writes came back this way, independently of the try before. Seen
  twice: `ApproveGoals` as the manager refused on all four tries within a
  couple of minutes of that manager's rating saves, then accepted straight
  after one more save. A token page with no token right after an empty 400
  means the session ended - run again.
- **Letter case.** Paths as the UI calls them: lowercase `/hr/pmsv10/...`.
- **Token pages:** `/hr/pmsv10/performancecycle?mode=create` (cycle
  templates, needs cycle-admin rights), `/hr/pmsv10/updatehub` (assessment
  templates, any employee or manager).
- **Names:** created data reads like real demo data ("FY2027 Annual
  Performance Review"), never "test".

## Performance Cycle values

- **Setup:** `performanceMethod` "90"/"180"/"360" with `sourceTemplateId`
  1/2/3. `copy-from-previous` needs a draft target.
- **Evaluation rules:** goal groups need goals on and the custom goal type;
  the competencies step needs competencies on. Rating method ids differ per
  environment (hosted: 32-34, 88-104; 33 = grades 434 A ... 437 D).
- **Timeline:** the saves take `annualGoalAllocation` +
  `annualCompetencyAllocation`, totalling 100 when both are on. The template
  covers goals on, no intermediate stages, no calibration. `split-annual-cycle`
  breaks a later Save & Continue on the same cycle - throwaway cycles only.
- **Evaluators:** enabled weights total 100 (e.g. 20/50/30); 180/360 need
  self on, 90 needs it off. Toggling the reviewer role resets Participants.
- **Competencies:** `assign-competencies` (one designation) ->
  `save-weights` (`profileId` from assign; area weights 100 per profile,
  competency weights 100 per area) -> `continue-competencies`.
  `copy-from-wizard` re-seeds everything - never on a weighted cycle.
- **Participants:** a list of `{empNumber, isActive, managerEmpNumber,
  managerAssignmentType, reviewer1EmpNumber, reviewerAssignmentType,
  sortOrder}`, assignment type `immediate_manager` or
  `individually_assigned`. It replaces the whole list.
- **Goal groups:** `goal-groups-save` first turns the preview groups'
  negative ids into real ones; continue needs weights totalling 100 and a
  rating method on each enabled group (tied to hosted's two wizard groups).
- **Configurations:** 12 keys, "0"/"1". `allowEvaluationOverlap` must be "1"
  when a participant is in another active cycle; `goalRevisionEnabled` "1"
  is what makes `revise-goal-plan` possible.
- **After publish:** `manage-toggle-publish` refuses drafts;
  `manage-delete` accepts drafts only.

## Assessment values

- **Participant id:** `SELECT cycle_participant_id FROM
  PeoplesHR.perf_cycle_participant WHERE performance_cycle_id = <cycle> AND
  emp_number = '<appraisee>'`. Templates on the flow take it as
  `participantId`; the requests do not send it.
- **Goals:** at least one per enabled group, weights 100 per group. In
  `upserts` a `null` `participantGoalId` INSERTS a goal every time it is
  sent - re-send existing goals with their ids.
- **Revision:** only between acknowledging the goal plan and the annual
  stage being opened (179000147 after), and only with goal revision on
  (179000146).
- **Rating ids:** `open-annual-stage` (as the employee) creates the annual
  goal and competency ids; manager and reviewer rate the same ids. Goals take
  the grade item id, competencies a mark 1-5. Every approver needs a mark on
  every goal and competency (179000019).

```sql
-- goal ids at a stage
SELECT g.participant_goal_id, g.goal_title, g.cycle_goal_group_id
FROM PeoplesHR.perf_cp_goal g JOIN PeoplesHR.perf_cp_goal_plan p ON p.goal_plan_id = g.goal_plan_id
WHERE p.performance_cycle_id = <cycle> AND p.timeline_stage_id = <stage> AND p.participant_emp_number = '<appraisee>';

-- competency ids at a stage
SELECT c.participant_comp_id, c.competency_name
FROM PeoplesHR.perf_cp_comp c
JOIN PeoplesHR.perf_cp_comp_area a ON a.participant_comp_area_id = c.participant_comp_area_id
JOIN PeoplesHR.perf_cp_comp_group g ON g.participant_comp_group_id = a.participant_comp_group_id
JOIN PeoplesHR.perf_cp_comp_plan p ON p.participant_comp_plan_id = g.participant_comp_plan_id
WHERE p.performance_cycle_id = <cycle> AND p.timeline_stage_id = <stage> AND p.emp_number = '<appraisee>';
```

- **Roles:** manager and reviewer use the same `pms-assessment-manager-*`
  templates; the server takes the role from the participant row, and
  `actingRole` only settles it for someone who is both. The final approval
  is `reviewer-approve-goals`; there is no manual final-rating handler.
- **`submit-assessment`** is irreversible for the employee and has no turn
  check on the server - never re-send it on a submitted assessment.

## Send-backs (not stages)

A send-back takes a stage back to not done; the flow then shows it as `next`
again.

- **Goal plan:** `manager-reject-goal-plan` (`rejectComment`) -> the employee
  corrects it with `submit-goal-plan` (existing goal ids in `upserts`) ->
  `manager-approve-goal-plan`.
- **Manager changes a submitted plan:** `manager-save-goal-plan` edits a goal
  (recorded for the employee to acknowledge) or soft-deletes one;
  `manager-restore-goal` undoes a pending delete (179000148 if none).
- **Manager to employee:** `manager-flag-revision` (`stepKey`, comment),
  then `manager-reject-goals` -> the employee re-rates and runs
  `submit-assessment` again. A reject needs a flag by the same role first
  (179000020); `manager-clear-flag-revision` unflags, and the FDP step has
  its own flag pair.
- **Reviewer to manager:** the same two templates with `actingRole`
  "reviewer" -> back to the MANAGER, ratings kept; the manager's re-approval
  archives the reviewer's flag.

## Known gaps

- Several templates build fixed shapes (one designation, first area, two
  competencies; two goal groups; one timeline layout) because a template
  cannot build a list of objects from captures.
- The assessment flow covers a goal-planning stage plus the annual stage -
  not intermediate stages.
- Not templated: file uploads, and so the attachment deletes that need one.
- PMS server issues noticed while mapping (not fixed): `SubmitAssessment`
  has no turn check; `SaveGoalRating` and `SaveGoalRatingAsManager` do not
  check the goal belongs to the caller / appraisee; `SaveGoalPlan` can
  change a plan the manager is reviewing; `SaveFdpAsManager` answers an
  unknown entry id with a 500 instead of a 422.
