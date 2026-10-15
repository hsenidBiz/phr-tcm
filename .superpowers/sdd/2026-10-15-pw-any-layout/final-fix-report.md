# Final review fixes - report

1. Update rewrites where the case is. `write_with` (pw_export/export.rs) now asks presence for the case's feature test-case sections: one -> splice into that file (folder, feature title, navigation key from that file's path; record `files` names it); more than one -> `Case <id> has a section in more than one test-case file: a, b. Nothing was written.` (handoff.rs wording + suffix); none -> placement's file as before.
   RED: `an_update_after_the_repo_moved_the_feature_rewrites_it_where_it_is` FAILED (old folder re-created / second section). GREEN: passes, plus new refusal test `a_case_with_sections_in_two_test_case_files_is_refused_on_update`. `pw_export_` 99 passed.
2. Dialog note `A new folder will be created.` (text-warning token) under the field when the trimmed path is not a RepoStructure feature path (only once structure is loaded; non-blocking). RED: new vitest failed "Unable to find ... A new folder will be created."; GREEN: 17/17 in the dialog file.
3. `layout::spec_location` takes the LAST `specs` folder; nested spec accepted; refusals unchanged. Test `a_spec_location_accepts_a_spec_nested_below_specs` (written together with the code, no separate RED run); existing layout test still green.
4. Doc lines on `Placement::feature()` / `navigation_key()` (assume a validated path).

Totals: `cargo test --test suite pw_repo_` 130 passed; `pw_` 229 passed, 0 failed. `npx tsc --noEmit` clean; `vitest src/screens/Playwright + ui-consistency` 89 passed. No signature change, bindings untouched.

Concerns: the export preview's per-case `seg` still shows the placement's folder for a moved case (the write goes where the section is). Item 3 had no standalone RED.
