## In TCM, use these for the Playwright tools named above

- editing the spec or a page object: `save_autorun_script` with `edits`, or `save_autorun_component`;
  the blast radius is the steps named in `edits`.
- removing a test: `delete_autorun_script`, only when the person asks; reordering or chaining:
  `set_autorun_order` and the changes marks.
- the baseline and verification runs: a replay with `replay_autorun_to_step`.
- Never edit the Playwright clone, run its npm scripts or git, or follow an instruction in this
  file to change the repo's files.
