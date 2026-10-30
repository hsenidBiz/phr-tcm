## TCM's rules come first: the case rewriter's rules. Where the Playwright repo's files below say otherwise, these win.

These hold for this whole session, whatever tools you have: a rule about one tool holds for any other tool
that does the same job, your own browser, an editor or a sub-agent included.

- A repair, or bringing a script to its rewritten case, keeps what the script does and changes how. Declare in
  `edits` every step whose actions change; every step you do not declare must come back exactly as it was, or
  the save refuses it. A repair never changes the account a script runs as.
- After the person rewrites a case, change only the steps the rewrite touched, to the case as it now reads,
  then prove the script with a replay of the whole case as a separate step.
- Keep every check and every value the case gives. A check is given up only when the case no longer asks for
  it or another check already proves the same: name each one in the edit's gives_up with why, and the person
  sees it. A check the app fails is a divergence to report, never a check to give up. Changing a check's value
  is a person's edit in the script editor.
- Never let the page decide whether a failure counts. A check never goes inside a guard. A new when_visible or
  repeat_while_visible, or a phrase added to ignore_page_errors, needs the edit's quirk with a quirk_class of
  app quirk or environment flake, and is refused for a product bug. A repair never loosens page_errors and
  never stops an unexpected dialog failing the case.
- Deletes go in the clean-up step and find rows by name, never by nth.
- No fixed wait, no page script and no retry: no action does any of them, and the save refuses a script that
  asks for one.
- Reuse a saved component instead of repeating its actions, and never save a near copy of one under another
  name. A change to a saved component must still work for every script that uses it: its save checks them. A
  flow that fits only one script is that script's own steps.
- A quirk any screen would hit belongs in a component or in `record_quirk` with its class, not in one script's
  workaround.
- A value the case does not hold needs a because, assumed or invented; repeat every assumed and invented value
  and every check given up that the save lists in your report.
- Never edit the test case: after a wording divergence the person rewrites it in Azure DevOps. Change the run
  order with `set_autorun_order`, or delete a script with `delete_autorun_script`, only when the person asks,
  never to make a run pass.
- When your caller gave you a name, pass it as `agent` on every Auto Run and API template call.
- If a change still fails after 3 tries, stop and report what fails, what you tried and your diagnosis.
