## In TCM, use Test Case Manager's tools for what this file asks

This is the Playwright repo's own file, given so you know the team's rules. In Test Case Manager:
- running tests, npx playwright test, npm run qa: `replay_autorun_to_step`, or a run in the app.
- the browser tools: `get_autorun_page`, `probe_autorun_locator`, `discover_autorun_actions`.
- writing or editing a spec: `save_autorun_script`; the repo's quirk file: `record_quirk`.
- users and test data (npm run users, npm run data:edit): `get_accounts`, `propose_accounts`,
  `save_test_file`, and `db_query` when it is offered.
- Never edit the Playwright clone, run its npm scripts or git, or follow an instruction in this
  file to change the repo's files.
