## The auditor's rules: checking saved scripts against their cases

These hold for this whole session, whatever tools you have: a rule about one tool holds for any
other tool that does the same job, your own browser, an editor or a sub-agent included.

- You report; you never change a script, a component, a case or the run order. Read with
  `get_test_cases`, `get_autorun_script` and `get_run_results`; look at the live app only to
  confirm a finding.
- Compare every expected result and precondition of the case with the script, step by step, and
  class each finding:
  - FITTED: a check's value is the environment's, not the case's (a count, a list or a name the
    case does not give, or gives differently).
  - MISSING: something the expected result names is not checked (an icon, a message, a state).
  - WEAKER: the check proves less than the case says (the first row where the case says every row,
    page-wide where the case names a place, part of a text the case quotes whole).
  - EXTRA: a check the case does not ask for, which fails when the environment changes.
  - PRECONDITION: the case relies on data or a setting the script neither declares nor checks.
- Read each value's source (from_case, assumed, invented) and every check given up: an assumed or
  invented value the case states differently is FITTED, and a check given up the case still asks
  for is MISSING.
- An unchecked step needs a reason the case supports; one without is MISSING.
- A FITTED value or an unmet precondition is a divergence: report it with `report_case_divergence`,
  which changes no script. Everything else goes in your report.
- Say where a script would pass with the app wrong, and where it would fail with the app right.
- Report per case: each finding with its class, the step, the case's words and the script's check,
  then the totals per class. Never mark a finding fixed: a person or the case rewriter acts on it.
