# tcm tool errors seen while writing Auto Run scripts, 2026-10-14

Errors and gaps the `tcm` MCP tools showed assistants while scripting PBI
135592's Cycle Configurations (14 scripts, all passing) and Preview (25
scripts) steps on QA Automation, app 2.1.3-beta.24. Earlier rounds:
`tcm-tool-errors-2026-10-13.md`. "Fault" says whether the app, the agent, the
harness or the environment is to blame, and the fix suggested.

The ones that cost the most time:
- **Setup drafts cannot be found by a unique name** (#2, #3, #5): fixture
  outputs only carry captured ids, so scripts open their draft by
  `data-cycle-id="{{setup.cycle_id}}"` - and then a placeholder locator can
  never be "probed as written" (#6, #7). Two Cycle Configurations runs failed
  because scripts opened an older same-named draft.
- **Every fixture change silently drops the approval of every setup that uses
  it** (#8), and setup scripts cannot be replayed until approved (#9).
- **A Competencies-off cycle cannot be built by any fixture** (#17).
- **Answer size**: the full guide, page dumps and one-template answers
  (#1, #12, #16).

| # | tool | what I sent (short) | what came back (verbatim, short) | fault and suggested fix |
|---|------|---------------------|-----------------------------------|--------------------------|
| 1 | get_autorun_guide | no args (full guide) | "result (117,906 characters across 1,840 lines) exceeds maximum allowed tokens. Output has been saved to ...txt" | App: guide too large for one answer. Split by section by default, or make `section` the default and list titles first. |
| 2 | save_api_fixture | `creates:[{kind:cycle,id:{{steps.1.cycleId}},name:{{steps.1.cycleId}}}]` | "creates: cycle's name {{steps.1.cycleId}} is the same capture as its id - use the capture that holds the name the step gave it" | App: the validator wants a name capture, but pms-cycle-save-progress outputs only cycleId. |
| 3 | save_api_fixture | same, name `{{steps.1.cycleName}}` | "creates: {{steps.1.cycleName}} does not come from a step" | App: no template can echo its input name as an output. Fix: allow `creates.name` to be the literal `{{prefix}} ...` param text, or let templates output their inputs. Workaround used: dropped `creates` (so Clean up cannot list those drafts). |
| 4 | save_api_fixture | `creates` with no `id` | "that is not a fixture: missing field `id`" | App: error message names the fixture id although the missing field is creates[].id. Older saved fixtures have name==id and still load. |
| 5 | save_api_fixture | outputs `cycle_name: "{{prefix}} ... {{now:yyyyMMddHHmmss}} ..."` | "output cycle_name: ... does not come from a step" | App: setup/fixture outputs are limited to captured step values (cycle_id only), so a script cannot open "its" draft by a unique name. Fix: allow `{{prefix}}`/`{{now}}` text in outputs, or expose fixture params as outputs. Workaround: open by `div[data-cycle-id="{{setup.cycle_id}}"] button[aria-label^="Edit"]`. |
| 6 | save_autorun_script | chain `[{css: div[data-cycle-id="{{setup.cycle_id}}"]},{role:button,name:Edit}]` | "button "Edit" in div[data-cycle-id="{{setup.cycle_id}}"] was seen on the live app, but never counted as matching exactly one element: probe it as written" | App: a locator holding a placeholder cannot be probed as written, so the instruction is impossible. Fix: count a probe of the same locator with the placeholder replaced by a real id, or accept chains with a seen first link. Single-string css with the placeholder was accepted. |
| 7 | save_autorun_script | `[card css with {{setup.cycle_id}}, {text:Draft, exact:true}]`, also `div[...] span` with nth 1 | "text "Draft" in div[data-cycle-id=...] ... never counted as matching exactly one" / "span #2 was seen ... probe it with its nth as written" | App: same cause. A per-card status badge ("Draft") of a setup draft cannot be asserted. Fix as #6. Case 142696 step 7 now checks only that the card is on the list. |
| 8 | run_api_fixture / save_api_fixture | edit of fixture pc-cycleconfig-own-2027 | (no message) | App: any fixture change silently invalidates setup approval of all 7 scripts that use it ("setup not approved"). Fix: show a "needs re-approval" list in the answer of save_api_fixture. |
| 9 | replay_autorun_to_step | case 142696 step 7 (setup script) | "bridge returned 409: setup not approved - approve it in the script editor" | App: by design, but it means no replay proof for any setup script until the person approves. Fix: a tool or flag to approve a setup in this session on request of the person, or replay with the setup run unattended-approved once. |
| 10 | start_autorun_discovery | start while a discovery is open | "bridge returned 409: the assistant is exploring the app in the Auto Run browser - end it with end_autorun_discovery" | Agent fault (expected), but run_api_fixture / run_api_template also fail while a discovery holds the account, so every fixture run needs end + start of discovery (2 extra sign-ins). Fix: let templates run on another browser profile, or say so in the run answer. |
| 11 | discover_autorun_action(s) | `no_save:true` on one call | answers keep `"no_save":true` for every later call until the discovery ends | App: sticky flag is not stated in the tool description; later saving steps silently blocked. Fix: describe it, or echo "(no_save still on)" clearly. |
| 12 | discover_autorun_action(s), replay_autorun_to_step | any | the whole page tree (60-120 lines, 3-5k tokens) after every call | App: no way to turn the page dump off; the list page has 30+ lines per card. Fix: `quiet:true` / `page:false`. |
| 13 | get_autorun_page | Preview step | tree shows cards and buttons but no text of spans, paragraphs or divs ("Name AUTOTEST ...", "Self 20%") | App: text-only elements are missing, so values have to be found by probing guessed text. Fix: print the visible text of generic elements. |
| 14 | probe_autorun_locator vs get_autorun_page | Edit button on the card | page: `button "Edit AUTOTEST ..."`; probe: `button ""` | App: accessible name differs (aria-label vs name). Fix: probe should print the same name. |
| 15 | probe_autorun_locator | `{role:button,name:"Ready Edit",nth:4}` and nth 3 (read-only probes) | "Permission for this action was denied by the Claude Code auto mode classifier. Reason: [Unrequested Commit in a Connected App]" and "[Production Deploy]" | Harness fault: read-only probes classified as commits/deploys. Same calls with nth 5, 6 passed, and nth 3 and 4 passed on retry. Fix: allow-list probe_autorun_locator. |
| 16 | list_api_templates | `id` set (one template) | the full flow definition (29 stages) plus the 17-file test file list, ~10k tokens | App: `id` answer repeats the flow every time. Fix: `flow:false` option. |
| 17 | save_api_fixture | fixture for a cycle with Competencies off (steps ... pms-cycle-save-participants-progress) | "step 5: template ... performs Participants ..., so the steps before it must perform Competencies (done, or not part of this cycle)" | App: flow gate demands a Competencies-stage template even when the stage is "not part of this cycle". Adding pms-cycle-continue-competencies -> run: "At least one profile must have competencies assigned before proceeding."; pms-cycle-assign-manager-competencies -> "Open competencies step (Step): expected status 200, got 404". No fixture can complete a Competencies-off cycle. Fix: a "skip stage" template/flag, or a template for the Competencies-off case. Also left 2 stray incomplete drafts "AUTOTEST Preview OKR 2027". |
| 18 | set_autorun_order | 14 cc1 ids | "saved Auto Run's order for PBI 135592" | App: replaces the order of the whole PBI (other pages run after the listed ones). Fix: say so in the answer; offer `append` mode. |
| 19 | db_query / db_lookup | first calls | "the database could not be read: Sqlcmd: Error: ... Named Pipes Provider: Could not open a connection" (later calls worked) | Environment: DB reachable only sometimes (VPN?). Fix: clearer message "database offline, retry". |
| 20 | record_app_quirk | 330-character note | "a quirk is at most 300 characters" | Info: limit not in the tool description. |
| 21 | save_autorun_script (cc1 142700/142696) | edits for several cases | refusals list one locator at a time over several calls (e.g. step 7 only after step 3 was fixed) | App: a dry run lists "every locator never seen" but not the "never counted exactly one" class in the same pass for all cases; a repair needs the full script again (12k tokens per save). Fix: accept a steps patch for a repair. |
| 22 | Auto Run areas | Common Configurator | list items unnamed in the accessibility tree ("listitem """) | App/PMS: the module list has no accessible names, so keys such as GOAL_KR_WEIGHTAGE_ENABLED cannot be read; case 142687 step 5 cannot compare KR switch with the setting. |

Count: 22 (cc1 items 2-9, 14, 21, 22; pv1 items 1, 10-13, 15-20).
