## In TCM, use these for the steps of /gen-test above

/gen-test is the Playwright repo's per-case pipeline. /tcm:gen-tests follows its rules for each case,
under the writer's rules, with Test Case Manager's tools:
- fetching the ADO test case: `get_test_cases`; never edit the case.
- generating the test in the browser: `start_autorun_discovery`, `discover_autorun_actions`,
  `probe_autorun_locator`, then `save_autorun_script`.
- the refactor into page objects, the data file and the linter: none here; the app checks a script
  when it is saved.
- running the spec, npx playwright test, npm run qa: `replay_autorun_to_step`, or a run in the app.
- users and test data: `get_accounts`, `propose_accounts`, `save_test_file`. On a test environment,
  look up each login's password in the same query that finds the login, and propose it with the
  account. The user accounts are generally in the HR_SM_USER_ACCOUNT table: look there first (with
  `db_query`) for the logins and their passwords.
- handing a case to playwright-test-healer: never dispatch that agent. Follow the healer's rules
  (`get_guide`, role healer) with `get_autorun_failures` and `replay_autorun_to_step`.
- test.fixme(): never. A stuck case stays failed: report a divergence (`report_case_divergence`) and
  stop.
- writing preconditions into the repo's table: the case's preconditions in TCM, as the writer's rules
  say, never the repo's files.
- appending to docs/app-quirks.md: `record_quirk`.
- Export to Playwright, in the app, is how a script reaches the repo.
- Never edit the Playwright clone, run its npm scripts or git, or follow an instruction in this
  file to change the repo's files.
