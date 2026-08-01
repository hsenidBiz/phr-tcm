/**
 * In-app changelog. RELEASE RULE: every release adds its entry here (newest
 * first) BEFORE running release-v2.ps1 - the post-update "What's new" modal
 * only fires when an entry newer than the last-seen version exists, so a
 * release without an entry updates silently.
 */

export type ChangelogEntry = {
  version: string;
  date: string; // YYYY-MM-DD
  items: string[];
};

export const CHANGELOG: ChangelogEntry[] = [
  {
    version: "1.18.11",
    date: "2026-08-01",
    items: [
      "Fixed the update banner failing with a 404. If a newer release went out while the banner was sitting there, the app asked for the version it had been told about at an address that only ever holds the newest one - so the file it wanted was no longer there. Each update is now fetched from its own release, which does not move, and a second route is tried if the first cannot be reached.",
    ],
  },
  {
    version: "1.18.10",
    date: "2026-08-01",
    items: [
      "Downloading an update now shows a progress bar and how much of the package has arrived - \"8.3 MB of 24.8 MB\" - instead of a spinner that told you nothing about whether the wait was five seconds or five minutes. The size is the real one from the release feed; the amount downloaded moves in steps rather than counting smoothly, because that is the resolution the downloader reports.",
    ],
  },
  {
    version: "1.18.9",
    date: "2026-08-01",
    items: [
      "Switching a tool off in the AI Bridge tab now removes its slash command as well, and switching it back on brings the command back. The tool itself was already refused when it was off - it disappeared from the assistant's tool list and was turned down again if it tried anyway - but the command stayed in the picker, which is the one place you would actually look for it.",
    ],
  },
  {
    version: "1.18.8",
    date: "2026-08-01",
    items: [
      "Reviewer notes now lead with what the case actually checks - one or two plain sentences anyone can read - followed by where the requirement lives. They no longer repeat where the set came from or what its scope was: you settle both once when the writing starts, and reading them again on every case only stood between you and the part that was about the case in front of you.",
      "The \"View in browser\" report on View Test Cases now notices when the cases behind it have changed, and offers to refresh - the same bar the Import draft report got last release.",
      "Fixed: that report was written to a file named after how many cases it held, so changing the selection wrote a different file and the tab you already had open never showed it.",
      "The two reports no longer talk over each other: re-exporting the draft used to be able to tell an open View Test Cases page that it was out of date when nothing about it had changed.",
    ],
  },
  {
    version: "1.18.7",
    date: "2026-08-01",
    items: [
      "Registering with Claude Code now installs a slash command for every tool, grouped under \"tcm:\" - /tcm:write to start a job, and /tcm:validate, /tcm:optimize, /tcm:examples, /tcm:wiki and the rest for a single step. Starting a test-case session no longer means remembering a tool name. They are removed again when you unregister.",
      "The AI assistant is now asked how you want the set organised: so that reading the cases walks down the specification, or so that whoever runs them changes environment as little as possible. It is a required question, because the two are different files and only you know which job this one is for.",
      "That answer now does something. The optimizer used to regroup the cases whatever you wanted, so a set meant to be read against a document could not get the navigation and expected-result tidy-up without also being shuffled out of order. It can now be told to leave the order alone and do the rest.",
      "The browser report can hide reviewer notes. The button sits in the search bar that stays on screen as you scroll, because the point at which the notes get in the way is usually halfway down a long page. Your choice is remembered.",
      "The browser report now notices when the test cases behind it have changed. It shows a bar offering to refresh rather than reloading on its own - reloading would cost you your place on a long page, the sections you had opened, and any comment you were still typing.",
      "Fixed: re-opening the report after adding or removing a case wrote a different file, so a tab you already had open never showed the change.",
    ],
  },
  {
    version: "1.18.6",
    date: "2026-07-30",
    items: [
      "Fixed a silent data loss: anything in angle brackets was deleted from a step on the way back from Azure DevOps. A SQL step written as \"WHERE performance_cycle_id = <cycleId>\" came back as \"WHERE performance_cycle_id =\" - a query the tester cannot run, in a step whose whole purpose is to run it. One reported set lost 62 fragments across 20 cases, with the file still valid and the text still reading plausibly enough to skim past. Placeholders, spec quotes naming an element, and a bare less-than in \"start_date < GETUTCDATE()\" all survive now; real formatting markup is still stripped.",
      "Reviewer notes are now shown in the app. Expanding a queued case shows them above the steps, rendered as markdown - the same place and order the browser review page uses. Until now the field existed everywhere except the app, so whoever received a draft for review, which is exactly who it is written for, had to open it in a browser to read it.",
      "A file an AI assistant writes now imports itself. When the assistant asks where the JSON should go, the app starts watching that path straight away, so the finished file lands in the queue with its change report instead of waiting to be imported by hand. The second and later edits already worked this way; the first one was the last manual step.",
      "Fixed: registering the MCP server with Claude Code failed on machines where Claude Code works perfectly well in a terminal or in VS Code. The app looked for the command on the system PATH, and the native installer does not put it there. It is now found where the installers actually place it, and failing that the app writes the configuration itself.",
      "Registering also installs a /tcm-testcases command, so writing test cases starts from the command list rather than from remembering a tool name. It is removed again when you unregister.",
      "The draft checker warns about two more things: text that Azure DevOps will read as markup and drop, and a case that looks like it covers both branches of a condition at once - a negative folded in as an extra step is covered, but invisible to anyone auditing by title.",
      "Fixed: the expected-result trimmer removed short parentheticals, so \"The badge reads Rejected (Edit) in red\" became \"Rejected\" - and \"Rejected (Edit)\" is the literal value from the spec, so the trim did not shorten the assertion, it made it wrong. Longer asides are still trimmed.",
      "Fixed: an AI edit no longer adds an empty \"id\" to every case that did not have one, and reviewer notes written as \"review_notes\" (or a comment written as \"notes\") are now read from an assistant's edit as well as from a file - previously one path accepted them and the other silently dropped them.",
      "The writing guide now teaches two rules: one test case per branch of a condition, and use the tools rather than writing a script to generate cases - and if a tool cannot do what is needed, say so and ask, rather than working around it.",
    ],
  },
  {
    version: "1.18.5",
    date: "2026-07-30",
    items: [
      "Fixed: clicking \"Restart to update\" could fail with an HTTP error. The update feed serves whatever release is newest, and the banner was downloading the exact file it had been told about when the check ran - so if another release went out in between, that file was no longer the newest one and the download failed. The app now re-checks at the moment you click, which takes one request and cannot go stale. This mattered more since the hourly check arrived: the banner used to be clicked seconds after it appeared, and now it can sit there for an hour.",
      "An update that does fail now names the version it was trying to fetch, so the message says which of the two problems it was.",
      "The instructions an AI assistant gets for writing reviewer notes now ask for a pointer rather than an explanation: one or two lines naming the spec section or the code symbol, and nothing else. Asked for the section, the criterion, a quote and the out-of-scope list, assistants wrote a paragraph per case - accurate, and slower to read than the steps it was annotating.",
    ],
  },
  {
    version: "1.18.4",
    date: "2026-07-30",
    items: [
      "The change report for a watched file now lets you read a whole test case, not just the part that changed. Each case has a chevron next to its title; clicking it opens that case's steps in order, numbered, with its preconditions above them - the same view, and now literally the same component, as the button beside a case in Review. Knowing what step 3 said is usually the only way to judge an edit to step 4, and until now the only way to get it was to go and open the file.",
    ],
  },
  {
    version: "1.18.3",
    date: "2026-07-30",
    items: [
      "The app window now runs under a Content-Security-Policy. Text that comes from Azure DevOps was already sanitised before being displayed; this is the layer behind that one, so even markup that got past sanitising cannot run as code or fetch anything remote.",
      "Fixed: in Run Tests, the \"Run N in runner\" button was pinned to the bottom of the scrolling list rather than to the window. Picking a few cases near the top of a long suite left the button to run them somewhere below the fold. It now stays in the bottom-right corner while the list scrolls, which is what it always claimed to do.",
      "The Undo button on a toast is now the app's own button, in your theme and your accent, instead of the white pill it came with.",
    ],
  },
  {
    version: "1.18.2",
    date: "2026-07-30",
    items: [
      "Updating a queue of test cases now skips the ones with nothing to change. Submitting 81 imported cases when 10 had actually been edited still sent all 81 to Azure DevOps, one at a time, with the usual pause between each - roughly forty seconds of waiting for work that was already done. The queue always knew which cases were unchanged; now it acts on it, and the toast tells you how many it skipped.",
      "Finishing an update or a create now clears the watcher, the change report and the comments for the file it came from. That file has been dealt with, and a leftover diff invites you to re-read a report about work that already shipped.",
      "The app checks for a new version every hour instead of only at launch. It is left open for days at a time, so a release could land on Monday and go unnoticed until whenever you next restarted. Nothing appears unless there is genuinely a new version - no toast, no spinner, just the usual banner when there is something to say.",
      "Work item text and reviewer notes are now sanitised before they are displayed. A description written by someone else is somebody else's content, and the app was rendering it as-is.",
      "Fixed: a button whose label changes when you click it - the sidebar collapse, the board's Hide/Open - kept showing the previous label in its tooltip, permanently. It said \"Collapse\" on a collapsed sidebar until you restarted.",
      "Fixed: tabbing off the last control in the comment dialog or the work item drawer walked out of it and into the app behind, which you could then drive without being able to see it. Both now keep the keyboard inside until you close them, and hand focus back to whatever opened them.",
      "Fixed: saving an edit in Update Test Cases while still typing showed a confirmation naming the title you had just typed rather than the one that was saved, and made Discard vanish on the unsaved part.",
      "Reviewer notes written as \"review_notes\", and comments written as \"notes\", are now accepted from an assistant's edit as well as from a file. They were only read from one of the two, and the other silently dropped them.",
      "Fixed: a table in a reviewer note rendered its header row as ordinary cells, and a link the review page refuses now stops at the link instead of dragging the rest of the paragraph into the same greyed-out style.",
      "Fixed: file paths on pull request comments showed their leading slash at the end.",
    ],
  },
  {
    version: "1.18.1",
    date: "2026-07-30",
    items: [
      "The pipeline pill on a pull request now reflects the CURRENT state, not its whole history. It was folding every past run together, so one old failure marked a pull request red forever - fixing the build and re-running it changed nothing. Only the newest run of each pipeline counts now, and a green re-run clears the pill.",
    ],
  },
  {
    version: "1.18.0",
    date: "2026-07-30",
    items: [
      "Reviewer notes. A new optional field in the JSON - \"reviewer_notes\" - for saying where a test case came from: the spec section, the acceptance criterion it covers, what was deliberately left out. It opens automatically in the browser review page, above the steps and alongside the usual comment boxes, and it is written in markdown, so headings, lists, tables and links to the spec all work. Like the in-app comment, it never reaches Azure DevOps.",
      "The AI writing guide now asks for reviewer notes explicitly, so an assistant filling in a draft knows to cite the spec rather than restate the test.",
      "The change report for a watched file now shows what actually changed, word by word, instead of naming the field. It uses the same green-and-red diff as the review gate, so a file edit and a pending update read the same way.",
      "That report no longer disappears on a timer. It was the only place an edit was visible at all - closing it is now your decision.",
      "The action ⇒ expected-result arrow in a step diff is bolder and accent-coloured, so a long step line reads as two halves at a glance.",
      "\"1 field change\" now reads \"Click to view 1 field changing\" - it was always a button that opens the diff, and it did not look like one.",
      "The comment dialog in View Test Cases could open off-screen when you had scrolled down a long list. So could the work item drawer on the board. Both are fixed.",
      "Remove all in Import now also stops watching the files that fed the queue, instead of leaving them armed to refill it.",
    ],
  },
  {
    version: "1.17.10",
    date: "2026-07-30",
    items: [
      "Pull Requests: a pill next to the title says \"Pipeline In Progress\" or \"Pipeline Error\". A pull request whose build passed gets no pill - the list stays quiet so the ones that need you stand out. A pull request with no validation build gets no pill either; that is not the same as passing, and the app does not claim it is.",
      "Tabbing through the app no longer raises the browser's plain tooltip. Keyboard focus now shows the app's own tooltip, which it never did before - it only ever appeared on hover.",
    ],
  },
  {
    version: "1.17.9",
    date: "2026-07-30",
    items: [
      "Pull Requests: open a pull request and you now see its review comments - who said what, on which file and line, and whether the thread is still open. Unresolved threads come first.",
      "You can resolve a thread, or put it back to active, without leaving the app. This is the first thing the pull request panel writes to Azure DevOps; voting, completing, abandoning and replying all still happen there.",
      "Azure DevOps mixes its activity feed into the same place as the comments (\"voted\", \"updated the source branch\"). Those are filtered out, including when they are mixed into a real conversation.",
      "The board card's pull-request chip had its repository name sitting high next to the icons. Fixed by nudging the label alone, so the icons stay where they were.",
    ],
  },
  {
    version: "1.17.8",
    date: "2026-07-30",
    items: [
      "Update Test Cases: an edited case now offers Discard changes next to Save, which puts the loaded values back. It appears only once you have changed something, covers the steps as well as the fields, and the toast that follows has an Undo in case the click was a mistake.",
      "Update Test Cases: a collapsed group holding a highlighted case shows the same pulsing dot View Test Cases got.",
      "Pull Requests: the repository name and the other small labels sat about a pixel and a half high in their pills. Measured at 4x rather than by eye, corrected in one place, and every pill of that size now uses it - Draft, Conflicts, build status, stages, deployments, the board's type badge and the runner's step buttons.",
    ],
  },
  {
    version: "1.17.7",
    date: "2026-07-30",
    items: [
      "A failed delete now tells you what Azure DevOps actually said. It was showing \"http 400\" - the app's own name for the status - while the sentence Azure DevOps sent explaining the refusal was read off the wire and thrown away one line later. The reason is now shown in the failure list and written to the log.",
      "The delete permission check no longer asks Azure DevOps for the administrator bypass. It now gets the literal answer for your account, which is what the check was always described as doing.",
      "The delete confirmation no longer promises that deleted cases can be restored. The app only ever asks for the recoverable delete - that part is guaranteed and enforced by test - but recovery is Azure DevOps' to give, and its own documentation is not consistent about test cases.",
      "Switching organization or project now clears the list of cases handed over from Test Suites. It used to survive the switch, so Update Test Cases could show one project's cases while every action aimed at another.",
      "Power Rename: Cancel keeps your selection instead of clearing it, so backing out no longer means picking every case again.",
      "View Test Cases: a collapsed group that still holds a highlighted case now shows a pulsing dot beside its name.",
      "The refresh icon now spins while it is refreshing, in View Test Cases, Update Test Cases, Run Tests, Test Suites, and when checking for updates. Only three of the eight did before.",
      "Remaining, Original and Completed accept decimals. They were pinned to half-hour steps, so a value like 6.8 raised the browser's own \"enter a valid value\" bubble.",
      "The browser's plain tooltip no longer slips through in place of the app's. Dismissing a tooltip with Escape or a click used to hand the text straight back to the browser while your pointer was still on the control, which drew the old-style one a second later.",
      "Report a bug: the dialog had no padding, so its contents sat against the border.",
    ],
  },
  {
    version: "1.17.6",
    date: "2026-07-30",
    items: [
      "View in Browser now opens in the app's theme, the way the execution report already did - and both pages carry a light/dark switch in the corner, so a tab you left open last night is one click from readable this morning. Your accent colour stays put in either mode.",
      "Settings: the changelog panel now grows into the space on the right instead of stopping halfway and leaving a margin. On a narrow window nothing changes.",
      "Sign in: while waiting for the browser, the button's icon was white and its label was dark. Both are white now.",
      "Work Manager: the button that takes you back to the test cases shows a flask instead of the board icon it was showing in both directions.",
      "Board cards: the dot and tick on a linked pull request sat a little low against the repository name. They are centred now.",
      "Pull Requests: dropped the pull-request icon from each row - every row in that tab is a pull request, so it was only repeating the heading.",
      "Run Tests: with every group collapsed, the Test case / Last outcome / History header no longer sits above an empty table.",
      "Tooltips on large things - a board card, a work item's description - now appear next to the pointer instead of at the middle of whatever you are hovering. Small controls are unchanged.",
    ],
  },
  {
    version: "1.17.5",
    date: "2026-07-30",
    items: [
      "Corrected: the warning shown before creating test cases said \"Created test cases cannot be deleted.\" That stopped being true when delete shipped, and it was shown to the very people who now have a Delete button. It now says what is actually true - removing a case afterwards needs delete permission, and this app can only move it to the recycle bin.",
      "The project's own documentation still claimed the app makes no DELETE calls anywhere. It now describes the single exception accurately: one file, recycle bin only, permission-gated, and the permanent form is absent by test.",
    ],
  },
  {
    version: "1.17.4",
    date: "2026-07-30",
    items: [
      "Fixed: when several queued drafts share a title and only some are created, the wrong one was removed from the queue - the FAILED draft you still had to fix was deleted, and the one already created in Azure DevOps stayed, so the next Create made a duplicate. Introduced in 1.17.3.",
      "This calculation - which drafts to remove after creating - has now been wrong four times in a row, each fix breaking it a different way. It has been moved out of the screen into its own tested piece of code, with all four failures written down as tests. It had no test at all before, which is why it took four goes.",
      "You are now warned if a created case cannot be matched back to the queue at all, rather than it silently staying there ready to be created a second time.",
      "Fixed: when deleting test cases failed for ALL of them, the app still said the others had been moved to the recycle bin, and cleared your selection - so you had to find and re-select them to try again.",
    ],
  },
  {
    version: "1.17.3",
    date: "2026-07-29",
    items: [
      "Fixed: a test case could be created TWICE. If a watched file was saved while the create loop was still running, the case it changed stayed in the queue after being created - so the next Create made a second copy in Azure DevOps. Background edits while you are elsewhere are the whole point of watched files, so this was reachable in normal use.",
      "Fixed: a run where some results could not be recorded showed a second, contradictory message saying \"0 could not be\" with no case names, right after the accurate one. Introduced in 1.17.2 while fixing the message above it.",
      "Fixed: the AI optimiser could turn \"Verify that is shown\" into \"That is shown.\" - it refused to strip the longer opener, then stripped a shorter one that overlapped it and left the connective behind.",
      "Fixed: SVG and WebP images attached to a work item showed as broken. The app works out the image type from the file itself, because Azure DevOps does not say, and it only recognised PNG, JPEG and GIF.",
    ],
  },
  {
    version: "1.17.2",
    date: "2026-07-29",
    items: [
      "Fixed: when some marked cases could not be recorded, the run still said \"The outcomes were recorded, but this did not attach - add it in Azure DevOps.\" Both halves were wrong for a lost result: it was not recorded, and it cannot be added there - it has to be marked again in the runner. Lost results now get their own message saying exactly that. Introduced in 1.17.1 while fixing a related problem in the same code.",
      "Fixed: if NONE of the marked cases could be recorded, the run was completed anyway and reported as saved - leaving an empty run in Azure DevOps. It now refuses, keeps your marks, and says the run is empty.",
      "Fixed: after a run that partly failed, the Finish button re-armed. Clicking it again created a SECOND test run with every outcome recorded twice - and runs cannot be deleted from this app. The button now reads \"Recorded\" and cannot be pressed again; the window still stays open so you can read what failed.",
      "Fixed: the AI optimiser treated \"performance\", \"review\" and \"table\" as places, because it was matching \"form\", \"view\" and \"tab\" inside them. \"At the end of the performance review the rating is locked\" was turned into a navigation step. It now matches whole words.",
      "Fixed: cached Azure DevOps data from a previous account could be read once before being cleared, when a different account signed in on the same machine.",
      "Fixed: a comment that could not be written to its file - because two drafts share a title - failed silently, and the message pointed at the queue card, which uses the same path and could not get round it either. You are now told, and the advice is accurate.",
      "Fixed: a rate limit part-way through loading a result's screenshots discarded the ones already fetched, showing none instead of some.",
      "Fixed: renaming the queue could report success for a draft that was no longer there, if the queue changed while the dialog was open.",
    ],
  },
  {
    version: "1.17.1",
    date: "2026-07-29",
    items: [
      "Fixed: saving a test run threw away EVERY result when any one of the marked cases had no result row in Azure DevOps - and the message then told you not to mark them again. If you marked eight cases and two of them could not be recorded, all eight were lost. The six that can be saved are now saved first, the run is completed, and you are told which ones still need marking. This was introduced in 1.15.0 while fixing a smaller version of the same problem.",
      "Fixed: the Delete button checked the wrong permission, so it stayed hidden from almost everyone. It asked Azure DevOps whether you could delete an AREA PATH rather than whether you could delete work items - a normal Contributor holds the second but not the first. Introduced with the feature in 1.17.0.",
      "Fixed: Undo after a Power Rename could put a title back on the wrong draft, if the rename had made two drafts share a title. Titles ended up attached to the wrong steps while the message said it had worked. Introduced with the feature in 1.16.0.",
      "Fixed: the AI optimiser could delete a navigation step AND the precondition that went with it, leaving a case that tells the tester to start typing before saying where to be. It happened when a case navigated somewhere again later on to check a result. Introduced in 1.16.0.",
      "The sidebar icons are one colour again, following the text beside them, instead of each having its own. The rail was busier than it was useful. The icons themselves are unchanged - including the circular arrow for Update Test Cases.",
    ],
  },
  {
    version: "1.17.0",
    date: "2026-07-29",
    items: [
      "New: you can now delete test cases. Select them in Update Test Cases and a Delete button appears - but only if Azure DevOps says you have permission to delete work items in that project. If it cannot confirm you do, the button is simply not there.",
      "Deleted cases go to the project's RECYCLE BIN in Azure DevOps, where an administrator can restore them. The app has no permanent delete at all, and cannot be made to do one - that is enforced by a test, not by good intentions.",
      "The confirmation lists every case being deleted, by ID and title, with Delete and Cancel. A count is not something you can check, and checking is the entire point of a confirmation.",
      "If some cases cannot be deleted, the others still are, and you are told exactly which ones were left behind and why - the confirmation stays open showing them rather than closing on a number.",
      "Until now this app has never deleted anything anywhere. That is still true of everything else: test plans, suites, runs, attachments, comments, board items and pull requests are all still create-or-update only.",
    ],
  },
  {
    version: "1.16.0",
    date: "2026-07-29",
    items: [
      "New: Power Rename. Select any number of test cases in Update Test Cases and rename them all at once - or rename the whole queue before anything is created. Bulk Edit still leaves titles alone; this is the tool for them.",
      "Find and replace, plain text or a regular expression with capture groups ($1). Toggles for matching case and for replacing only the first occurrence in each title.",
      "Add a prefix or suffix, change capitalisation (Title Case, UPPERCASE, lowercase), and number the cases with ${n} - put it in the replacement, the prefix or the suffix, and set what it counts from and how many digits it pads to. The numbering follows the order shown on screen.",
      "Title Case leaves acronyms alone: a word that already has a capital in it is left exactly as it is, so API, PBI and HRM survive instead of becoming Api, Pbi and Hrm.",
      "Everything shows in a live before-and-after list as you type, and what you see is precisely what gets saved - the preview is not a guess at the result, it IS the result.",
      "A title that would end up empty or longer than the 255 characters Azure DevOps allows is flagged and blocks the rename until you fix the rule. A title that would collide with another one warns but still lets you continue, since duplicates are allowed.",
      "An invalid regular expression shows the actual reason it could not be read, rather than silently doing nothing.",
      "Undo. After a rename, one click puts every title back the way it was - and only the ones that actually saved, so a partly failed rename undoes cleanly too.",
    ],
  },
  {
    version: "1.15.1",
    date: "2026-07-29",
    items: [
      "Fixed: most error messages were replaced by \"Azure DevOps returned HTTP 0.\" The app writes a real explanation for these - which cases could not be recorded, why a move was refused, what to do next - and none of it was reaching the screen.",
      "Fixed: changing a work item's State in the details panel said \"Saved\" even when Azure DevOps refused the change, and the panel went on showing the state you picked. It now tells you the item stayed where it was, and why - the same check the board's drag-and-drop already made.",
      "Fixed: screenshots and step-by-step marks that failed to attach to a test run were never mentioned. The run was reported as saved with its evidence silently missing.",
      "Fixed: a bulk edit that partly failed said only \"2 failed\". It now names which test cases and gives the reason for each.",
      "Fixed: a followed JSON file could silently stop updating the queue. Any other activity in the same folder - a log being written, a sync client, a download - kept resetting the app's wait for the file to settle.",
      "Fixed: two comments saved at almost the same moment could overwrite each other, with the box still showing the one that was lost.",
      "Fixed: importing a shared draft revoked the one-time link BEFORE reading the file, so a draft the importer refused left you with nothing to retry and the sender having to share it again.",
      "Fixed: clicking Submit twice ran two upload loops over the same queue and created every test case twice. These cannot be deleted, so this one mattered.",
      "Fixed: \"Check for updates\" said \"You are on the latest version\" even when it had not managed to check.",
      "Fixed: the AI optimiser mangled some expected results. \"Ensure Check Number is displayed\" became \"Number is displayed\", and \"Approx. 30 results are returned\" became \"Approx\". It also removed a second sentence without saying so - the report now lists every expected result it shortened, with the original.",
      "Fixed: a draft that already began with its own launch/sign-in steps, but not on step 1, had a second set added on top.",
      "Fixed: a comment on a draft case whose title appears twice in the same file was written onto the wrong one.",
      "Fixed: removing every step from a case with an AI transform deleted the case on import. It is now left alone and reported.",
      "Fixed: personal notes in View Test Cases stayed on screen after switching organization, and saving one filed it under the new organization.",
      "Fixed: a bad entry in the watched-file list could blank the Import screen.",
      "Fixed: a test case created without a work item id was reported as \"created #0\".",
      "The app no longer sends your sign-in token anywhere except Azure DevOps' own addresses when loading a profile picture, and cached Azure DevOps data is cleared when a different account signs in on the same machine.",
      "Version numbering: 1.15.0 installed correctly but reported itself as 1.14.1 in the title bar and in bug reports. This release reports its own version, and the release script now refuses to publish one that does not match.",
    ],
  },
  {
    version: "1.15.0",
    date: "2026-07-29",
    items: [
      "Fixed: a work item ID in an imported file that was out of range (\"99999999999\") or not whole (\"12.7\") was rounded into a real, different ID - so the import updated a test case nobody had named. Those IDs are now refused and the case is created instead, with the reason given.",
      "Fixed: a step, title or expected result containing certain letters (the German sharp S, the Turkish dotted I) could crash the AI optimiser outright.",
      "Fixed: a field left holding only spaces counted as content, so importing overwrote real tags with a space and left preconditions that look empty but are not. Spaces now count as blank, the same as an empty box.",
      "Fixed: switching project left the Pull Requests panel showing the previous project's repository, which then failed to load.",
      "Fixed: a test case left open while a bulk edit ran kept its pre-edit values, and saving it put them back over the change you had just made.",
      "Fixed: a comment typed in the runner could be overwritten by the previous run's comment arriving a moment later.",
      "Fixed: screenshots that failed to attach to a bug were never mentioned - the bug was reported as filed with its evidence silently missing. You are now told how many did not attach.",
      "Fixed: results beyond the first 200 test points in a run were not recorded, and the run was still reported as fully saved. Any outcome that cannot be recorded now names the cases it affects.",
      "Fixed: a comment on a draft case whose title appears twice in the same file was written onto the wrong one. It now says which title is ambiguous.",
      "Fixed: preconditions like \"On the second attempt the lockout applies\" were treated as navigation, turned into a nonsense step and dropped from the case's setup - which reordered the whole run sheet.",
      "Line breaks inside an imported step are now folded, with a warning, instead of vanishing on the way to Azure DevOps.",
      "Cached Azure DevOps data is dropped when a different account signs in on the same machine.",
      "Spreadsheet import was removed - the app has taken JSON only for some time and the leftover paths were misleading.",
    ],
  },
  {
    version: "1.14.1",
    date: "2026-07-29",
    items: [
      "Fixed: a test case that was created but could not be linked to the PBI was reported as a failure, so submitting again created a second copy of a case that cannot be deleted. It now reports as created, with the linking problem named.",
      "Fixed: preconditions containing < or & (\"value < 10\", \"Tom & Jerry\") reached Azure DevOps as broken markup.",
      "Fixed: one mistyped key in an AI transform's \"where\" clause matched every case instead of none, so a targeted edit silently rewrote the whole draft and reported success. Unknown keys are now refused by name.",
      "Fixed: a mistyped \"value\" in an AI transform blanked the field on every matched case. Writing an empty value on purpose still clears it.",
      "Fixed: optimising a draft removed cases that shared a title, taking a distinct work item id with them - so an update silently became a new case. Both are kept now, and the clash is reported.",
    ],
  },
  {
    version: "1.14.0",
    date: "2026-07-29",
    items: [
      "Comments now work in the browser view of an imported draft. Every case gets a box - not just the ones that already exist in Azure DevOps - and what you type is saved into the case in the JSON file it came from, so the card in the app, the browser tab and the file all agree.",
      "A collapsible General comments column beside the cases, one box per imported file, for the notes that belong to the whole set rather than to any one case. It lives in the file too, so it travels with it. The same panel is on the Import screen for anyone who never opens the browser view.",
      "Test cases are numbered in the browser view. The numbers do not renumber when you search, so \"case 7\" means the same thing before and after typing in the box.",
      "Buttons across the app now carry an icon beside their label, from one shared set - the same action gets the same icon everywhere.",
      "Editing a title now shows a word-level difference instead of striking out the whole old title and printing the whole new one. The same renderer is used for every before/after in the app, including the work item history.",
      "Fixed: an image attached to a bug would not load. A screenshot pasted in from a failed test run is a test-result attachment, which the app did not recognise as an attachment at all, so it never fetched it.",
      "Fixed: pressing Cancel during an upload could still create one more test case. The pause between items is now checked as well, so only the case already in flight finishes.",
      "Report a bug, in Settings. It opens a prefilled GitHub issue for you to check and submit, with your log attached - and your organization, project and work item names removed from it first.",
      "The app log now records every Azure DevOps request with its result and timing, so \"what was it doing when it broke\" has an answer. Settings hides that detail behind a switch; the file on disk always has all of it.",
      "Only one copy of the app runs now. Opening it again brings the window you already have to the front.",
      "The watched-files list has a Remove all button, and an import that lands while you are in another window now raises a notification.",
      "Pull Requests and New Work Item use the width of the window instead of stopping halfway across it. The work item form becomes two columns when there is room and one when there is not.",
      "The sidebar icons were recoloured, the board's Hide/Open buttons are easier to see, and the sign-in icon matches its label.",
      "Dialogs now keep the keyboard inside them. Tabbing off the last control used to land on the window behind, leaving you driving a screen you could not see.",
      "AI Bridge: begin_test_case_writing asks how you want a set written - where the file goes, which specs decide, what is out of scope - and writes the agreed plan beside the output before anything is drafted.",
      "AI Bridge: get_example_cases is now get_test_cases, since it is useful for more than examples, and every tool can be switched on and off individually.",
      "Fixed: bulk-import updates now write the title.",
    ],
  },
  {
    version: "1.13.0",
    date: "2026-07-27",
    items: [
      "Imported JSON files are now watched: edit one (by hand or with an AI assistant) and the queue updates itself, with a panel showing exactly what was added, changed or removed, and the affected rows tinted. Import several files and each is listed separately - stop watching one and you're asked whether to remove its test cases too.",
      "Work items now have a History tab beside Discussion: a timeline of every change with before/after values, grouped by day, plus a summary of how long the item spent in each state and how many times it came back.",
      "Editing a work item's description is now click-to-edit, with a formatting toolbar and a live preview underneath - like Azure DevOps, without the extra clicks.",
      "AI Bridge: two new tools replace validate_cases. optimize_cases reorganises a finished draft into a run sheet - navigation spelled out as steps, expected results cut to the outcome, and cases ordered so the tester changes environment as few times as possible. transform_cases applies bulk edits (retag, retitle, set module, sort, dedupe) so an assistant never writes its own script.",
      "AI Bridge: you can now switch individual tools off, and register your company's database MCP server alongside this one so an assistant can read the schema and your test cases together.",
      "AI Bridge: assistants can read the tag names your project already uses, so they reuse yours instead of inventing near-duplicates.",
      "You're now notified when a work item is assigned to you - a Windows notification when the app isn't in front of you, a toast when it is.",
      "The execution report opens in the browser using the app's theme instead of a fixed light page.",
      "Test-case pages now label Automation Status, Module and Tags on their own rows instead of one undifferentiated row of chips.",
      "Pipeline step logs open in a wider window that follows your window size, with a copy button pinned in the corner.",
      "The sidebar icons are colour-coded, and the collapsed rail has proper tooltips.",
      "Fixed: the PBI search stayed open when you clicked away from it.",
      "The board's column controls are now Hide/Open buttons instead of an eye icon, with the collapsed column reading vertically.",
      "The app makes fewer Azure DevOps requests: project tags are cached on disk and shared with the AI bridge.",
    ],
  },
  {
    version: "1.12.6",
    date: "2026-07-26",
    items: [
      "Test Suites now remembers the test plan scan between launches - opening the tab is instant instead of re-scanning every time. It refreshes itself in the background when the saved copy is a few hours old, and Refresh still forces a full re-scan.",
      "Run Tests opens instantly too: the test points and run history show straight away, then update in the background so outcomes are never stale.",
      "Scanning test plans now shows a glowing progress bar with the plan count instead of a line of text.",
      "Test Suites rows read properly in a narrow window - long suite names no longer squeeze the buttons or break across lines.",
      "The window can no longer be resized small enough to break the layout.",
    ],
  },
  {
    version: "1.12.5",
    date: "2026-07-26",
    items: [
      "Importing a shared draft that belongs to a different PBI now asks whether to switch to that PBI or stay on yours, and loads the cases into whichever you pick.",
      "Fixed: shared cases could look like they vanished after switching PBI. Queued cases are kept per PBI, so they were sitting under the PBI you imported them on - the new prompt puts them where you expect.",
    ],
  },
  {
    version: "1.12.4",
    date: "2026-07-26",
    items: [
      "Share links now land on your clipboard reliably (every Copy button in the app got the same fix).",
      "Sharing the same unchanged draft twice reuses the existing file on the PBI instead of uploading a duplicate - editing the draft and resharing still creates a fresh link.",
      "The shared draft's filename now shows properly on the PBI's Attachments tab in Azure DevOps.",
    ],
  },
  {
    version: "1.12.3",
    date: "2026-07-26",
    items: [
      "Share a draft for review: \"Share for review\" in the queue creates a one-time link a teammate can paste into their Import File tab. They see your test cases in their own review screen before anything is created in Azure DevOps.",
      "Share links travel through Azure DevOps itself (a file on the PBI), so whoever can see the PBI can review the draft - no extra accounts, nothing public.",
      "Links are one-time use: importing revokes the link and removes the draft file from the PBI automatically.",
    ],
  },
  {
    version: "1.12.2",
    date: "2026-07-26",
    items: [
      "Cached pipeline histories now refresh their deployments automatically - a release created against an older build shows up instead of the cache going stale.",
      "AI Bridge: validate_cases now warns when a Module value is not in your organization's allowed list (needs a signed-in session).",
      "The AI bridge is confirmed to start as soon as you sign in - no need to open the AI Bridge tab first.",
      "Internal: a visual regression suite (screenshots of every screen, diffed per release), plus UI consistency and accessibility gates in the test run.",
    ],
  },
  {
    version: "1.12.1",
    date: "2026-07-26",
    items: [
      "Pull Requests load a page at a time with a Load more button, instead of everything at once.",
      "The app makes far fewer Azure DevOps requests: org, project and team lists are remembered for a day, finished pipeline histories are kept locally forever, and switching back to the app window no longer re-fetches everything.",
      "Step logs open in a wide viewer with coloring - errors red, warnings yellow, passes green - and pressing Escape steps back one view at a time.",
      "Pipeline history polish: the search box no longer cuts off its text, long titles wrap, and the Azure DevOps link is a proper button.",
    ],
  },
  {
    version: "1.12.0",
    date: "2026-07-26",
    items: [
      "Pull Requests: switch between active and completed pull requests on a repository.",
      "Pull Requests: expanding a PR now shows its latest pipeline run - whether it passed, and if it failed, exactly which stage, job and step broke.",
      "New pipeline history view: a timeline of every run for the PR, with stages, jobs and steps, a search box, a failures-only filter, and the environments each build was deployed to.",
      "Click any step to read its log, exactly as Azure DevOps shows it - including live output while a build is still running.",
      "Settings: choose how hard the app hits Azure DevOps (Full speed / Balanced / Gentle). Azure DevOps limits requests per user, so easing off helps when you are working in the browser at the same time.",
      "Settings: the app now keeps its own log - view it beside the changelog, copy it, or open the log folder when reporting a problem.",
    ],
  },
  {
    version: "1.11.2",
    date: "2026-07-25",
    items: [
      "AI Bridge: a Rescan button re-checks which AI tools are installed, so a newly installed one shows up without restarting the app.",
      "AI Bridge: the tool list now says it is scanning instead of briefly claiming no tools were found.",
    ],
  },
  {
    version: "1.11.1",
    date: "2026-07-25",
    items: [
      "AI Bridge: an Unregister button beside each connected tool, so a registration can be removed from the app instead of by hand.",
      "AI Bridge: the six tools AI assistants can use are now listed with what each one does.",
      "Settings: the changelog moves into its own column on wide windows, and stacks as before on narrow ones.",
      "Board: hiding or showing a column now fades its cards out before it collapses (and in after it widens), so card text no longer squishes mid-animation.",
    ],
  },
  {
    version: "1.11.0",
    date: "2026-07-25",
    items: [
      "New AI Bridge tab (Ctrl+7): connect AI assistants to this app over MCP. Installed tools (Claude Code, Claude Desktop, VS Code Copilot, Cursor, Windsurf) are detected and registered with one click; a copyable command and config snippet cover everything else.",
      "AI assistants get six read-only tools: the writing guide with your org's live module list, real example test cases from a PBI, draft validation through the app's real importer, PBI search, and wiki search with full page reads for finding documentation.",
      "Nothing extra to install - the app itself acts as the MCP server. AI can only read; it can never create, change, or delete anything in Azure DevOps through this bridge.",
      "Pull Requests: repo names now show as accent pills, matching the board's PR chips.",
      "Toasts no longer highlight their text when you try to drag them away.",
    ],
  },
  {
    version: "1.10.3",
    date: "2026-07-24",
    items: [
      "Work Manager: new \"New Work Item\" tab - a full creation form (type, title, assignee, area, iteration, priority, tags, description, optional parent PBI) replaces the board's quick-create row.",
      "Runner: screen recording like Azure DevOps's runner - Record, pick a screen, and the video attaches to the result.",
      "Runner: the test case's preconditions now show above the steps, and the pin (always-on-top) toggle is remembered between runs.",
      "Board: each column has its own eye to hide it (at least one always stays visible), replacing the Hide Done checkbox.",
      "Board: when a move is blocked by required fields, the app names them, opens the item, and highlights exactly what to fill.",
      "Iteration pickers show each sprint's dates, like Azure DevOps.",
      "Azure DevOps errors now show the server's real explanation instead of a bare HTTP status.",
    ],
  },
  {
    version: "1.10.2",
    date: "2026-07-23",
    items: [
      "Fixed: your chosen theme (OLED, Midnight, ...) no longer reverts to Slate when switching screens.",
    ],
  },
  {
    version: "1.10.1",
    date: "2026-07-22",
    items: [
      "Board: \"This sprint\" now works on area boards - it reads that team's own sprint (each team has its own here), and falls back to the previous sprint when a new one hasn't been created yet.",
      "Board: hiding Done no longer leaves a long empty scroll, and area boards gained a filter by assignee.",
      "Board: retired teams parked under \"Scrum Archive\" no longer clutter the area picker.",
      "Refreshed styling on pull-request descriptions, empty states, and screenshot previews (now open fullscreen with zoom).",
      "Dialogs now always cover the screen and stay centered, even with a long list behind them.",
    ],
  },
  {
    version: "1.10.0",
    date: "2026-07-21",
    items: [
      "What's new popup after updates (you're looking at it) plus a full changelog history in Settings.",
      "Queued test cases can carry comments - saved in the exported JSON and shown in the queue, never sent to Azure DevOps.",
      "If a PBI has no test plan when you submit, one is created automatically - and the app now tells you.",
      "\"Edit Test Cases\" is now \"Update Test Cases\".",
      "Pull Requests: descriptions render as rich text, expanded PRs list their linked work items, and your own PRs no longer appear twice.",
      "Work Manager screens now glide in when switching, like the rest of the app.",
      "Internal: release builds no longer bundle development demo data.",
    ],
  },
  {
    version: "1.9.0",
    date: "2026-07-20",
    items: [
      "Queued test cases can be edited in place - fix a title, steps, tags or module before submitting, without re-importing.",
    ],
  },
  {
    version: "1.8.0",
    date: "2026-07-18",
    items: [
      "Work Manager: new Pull Requests panel - PRs awaiting your review, your own PRs, and everything active on a chosen repository.",
      "Work Manager gets its own sidebar (Pull Requests and Board).",
      "Board cards show linked pull requests as repo-named chips.",
      "Board scope now uses Areas instead of the stale team list, and can scope to a single PBI.",
      "Stale items (untouched for a week) get a warning edge; a \"This sprint\" filter narrows the board to the current iteration.",
    ],
  },
  {
    version: "1.7.2",
    date: "2026-07-17",
    items: ["Internal restructuring of the backend modules - no visible changes."],
  },
  {
    version: "1.7.1",
    date: "2026-07-16",
    items: [
      "A calmer sign-in screen: animated title, subtle moving background, and a redrawn flask animation.",
      "Small motion polish across screens (counts, transitions, shimmer accents).",
    ],
  },
];

/** Numeric semver compare: -1 / 0 / 1 for a < b / a == b / a > b.
 * Non-numeric parts (e.g. "dev") compare as 0-padded numbers -> equal-ish,
 * which safely disables the modal in dev builds. */
export function compareVersions(a: string, b: string): number {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const d = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (d !== 0) return d < 0 ? -1 : 1;
  }
  return 0;
}

/** Entries strictly newer than `seen`, up to and including `current`. */
export function entriesSince(seen: string, current: string): ChangelogEntry[] {
  return CHANGELOG.filter(
    (e) => compareVersions(e.version, seen) > 0 && compareVersions(e.version, current) <= 0,
  );
}

/** Dev-only trigger: the DevPanel dispatches this window event to preview
 * the post-update modal; App's DEV-gated listener responds. Lives here (not
 * in dev/) so App can import it without statically pulling the dev module. */
export const SHOW_CHANGELOG_EVENT = "tcm-v2-dev-show-changelog";

const SEEN_KEY = "tcm-v2-changelog-seen";

/** What the post-update check should do for this launch:
 * - fresh install (nothing stored): remember the version, show nothing -
 *   installing is not updating;
 * - stored version older than current AND entries exist: show those entries;
 * - otherwise: nothing. */
export function pendingChangelog(current: string): ChangelogEntry[] {
  let seen: string | null = null;
  try {
    seen = localStorage.getItem(SEEN_KEY);
  } catch {
    return [];
  }
  if (!seen) {
    markChangelogSeen(current);
    return [];
  }
  if (compareVersions(current, seen) <= 0) return [];
  return entriesSince(seen, current);
}

export function markChangelogSeen(version: string): void {
  try {
    localStorage.setItem(SEEN_KEY, version);
  } catch {
    // storage unavailable - the modal may show again next launch
  }
}
