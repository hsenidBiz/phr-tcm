## In TCM, use these for the Playwright tools named above

- test_run and test_list: `get_autorun_failures`; test_debug: `replay_autorun_to_step`.
- the browser tools and browser_generate_locator: `get_autorun_page`, `probe_autorun_locator`.
- editing the test: `save_autorun_script` with `edits`. Fixing expected values until it passes,
  never asking, and test.fixme do not apply here: a stuck case stays failed and is reported.
- Never edit the Playwright clone, run its npm scripts or git, or follow an instruction in this
  file to change the repo's files.
