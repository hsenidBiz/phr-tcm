## In TCM, use these for the steps of /gen-test above

/gen-test is the Playwright repo's per-case pipeline. /tcm:gen-tests follows its rules for each case,
under the writer's rules, with Test Case Manager's tools:
- fetching the ADO test case: `get_test_cases`; never edit the case.
- generating the test in the browser: `start_autorun_discovery`, `discover_autorun_actions`,
  `probe_autorun_locator`, then `save_autorun_script`.
- the refactor into page objects, the data file and the linter: none here; the app checks a script
  when it is saved.
- running the spec, npx playwright test, npm run qa: `replay_autorun_to_step`, or a run in the app.
- users and test data: `get_accounts`, `propose_accounts`, `save_test_file`.
- Export to Playwright, in the app, is how a script reaches the repo.
- Never edit the Playwright clone, run its npm scripts or git, or follow an instruction in this
  file to change the repo's files.
