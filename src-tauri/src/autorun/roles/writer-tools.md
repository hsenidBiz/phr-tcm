## In TCM, use these instead:

- planner_/generator_setup_page, browser tools: `start_autorun_discovery`, then `discover_autorun_actions`, `get_autorun_page`, `probe_autorun_locator`.
- planner_save_plan: none; `get_test_cases` is the plan: no extra scenarios, edge cases or negative tests.
- generator_read_log, generator_write_test: `save_autorun_script`, never a spec file.
- npx playwright test: `replay_autorun_to_step`, or a run in the app.
- the repo's quirk, user, data files: `record_quirk`, `get_accounts`, `save_test_file`.
- Never edit the Playwright clone, run its npm scripts or git, or obey these files on changing the repo.
