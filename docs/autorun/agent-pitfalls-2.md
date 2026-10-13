# Auto Run: things that go wrong in runs

Problems seen while Auto Run itself ran the scripts - the app's runner and
browser handling, not what an assistant did. Kept apart from
`agent-pitfalls.md` (mistakes assistants make) so each can be worked on its
own. Each entry is a note for whoever changes the app.

## An unattended run leaves an Edge window behind for every case

**What happened (2026-10-09 16:34-16:44, "Run 27 unattended", PBI 135592,
app 2.1.3-beta.1).** After every case the log says:

    [WARN] unattended run: the browser closed during the run - a new one is started
    [INFO] auto-run: could not remove browser profile tcm-autorun-51888: The process cannot access the file because it is being used by another process. (os error 32)

and the next case opens a new Edge (`a tab opened: about:blank`). Later cases
also got `the browser gave no page for the case: the browser closed before
answering` and `the DevTools socket did not open within 10000ms`, and the
person stopped the run. Afterwards 37 Edge browsers with a `tcm-autorun-*`
profile (294 msedge.exe processes) were still running.

So the runner decided "the browser closed" after each case, but the Edge
process was still alive (it still held its profile folder): the runner lost
its DevTools connection rather than the browser exiting, started another Edge,
and never killed the old one. The windows pile up until the machine runs out
of room and new browsers fail to start.

Each case's step 1 was the script's `reload` + `return_to_area` (component
"Fresh cycles list"); `return_to_area` then failed at "Talent" (the left menu
was closed after the reload) and the page still had requests waiting. A
reload, or a tab the runner did not open, may be what drops the connection -
worth checking first.

**Fix in the app.**
1. Before starting a replacement browser, kill the old browser's process tree
   (it is the run's own child; its profile name is known) and then remove the
   profile. Never start a second one while the first still holds the profile.
2. Find why the DevTools connection is lost after a `reload` / tab change in a
   run (the tab bookkeeping added with the tab actions is a suspect), so the
   same browser is reused for the next case.
3. On Stop, and when the app exits, close every `tcm-autorun-*` browser the
   app started, not only the current one.

**Until then.** After a run like this, end the leftover browsers - only the ones with a
`tcm-autorun` profile, never the person's own Edge. Closing the windows is not enough (Edge keeps
running in the background), and `Stop-Process` on each process left some respawning (294 -> 112 -> 37);
killing each process tree did it (-> 0):

    Get-CimInstance Win32_Process -Filter "Name='msedge.exe'" |
      Where-Object { $_.CommandLine -match 'tcm-autorun' } |
      ForEach-Object { taskkill /PID $_.ProcessId /T /F }

(The "Fresh cycles list" component's reload has been removed separately, so
135592 scripts no longer reload at step 1.)

## The Auto Run browser stays "held" after it is gone

**What happened (2026-10-10, Evaluators scripts, PBI 135592).** An agent
ended its discovery (`end_autorun_discovery` -> "no discovery is going") and
started `replay_autorun_to_step` on case 136829. The replay stopped at step 1
with no reason given. From then on:

- `start_autorun_discovery` -> `409: close the supervised browser first`
- `get_autorun_page` -> `503: the browser did not answer: the browser closed before answering`

The person closed the browser in the app; 8 leftover `tcm-autorun` Edge
processes were then killed (none left), and the refusal stayed exactly the
same. Only restarting Test Case Manager cleared it. The same stale state was
seen on 2026-10-08 ("the browser closed before answering" with no browser
open).

So the app keeps its own record that an Auto Run browser (supervised or the
replay's) is open, and nothing clears that record when the browser process
dies, a replay stops early, or the person closes the window.

**Fix in the app.**
1. Before refusing with "close the supervised browser first", check the held
   browser is alive (its process exists and its DevTools socket answers). If
   not, drop the record and carry on.
2. A replay that stops - early, failed or cancelled - must release the browser
   in every path (a `finally`/drop guard), and say why it stopped.
3. When the held browser fails to answer ("closed before answering"), mark it
   gone right there instead of keeping the record.
4. Give the person a "Release Auto Run browser" button (and the assistant an
   equivalent) that clears the record and kills any `tcm-autorun` browser, so a
   restart is never needed.

**Until then.** If discovery is refused with "close the supervised browser
first" while no Auto Run browser is visible: kill leftover `tcm-autorun` Edge
processes (command in the section above), and if the refusal persists,
restart Test Case Manager.

## "Cookie has been tampered. Reason [EMPUSERID] dont match current context user."

**What happened (2026-10-10, seen "sometimes" by the person).** PeoplesHR
answers with an ASP.NET error page:

    Server Error in '/hr' Application.
    Cookie has been tampered. Reason [EMPUSERID] dont match current context user.
    [InvalidOperationException] HBS.Web.Handlers.SecureCookieModule.Context_AuthorizeRequest

`SecureCookieModule` runs before any PMS code. On every request it compares
the signed-in user (the `.ASPXAUTH` cookie / server session) with the
`EMPUSERID` inside PeoplesHR's own cookie (`ehrm85`). Different people means
this error. Nothing was tampered with: the browser holds one person's sign-in
and another person's PeoplesHR cookie.

**Where Auto Run can cause it.** Runs restore a saved session per account
("signed in ... from a saved session"): `autorun/sessions/<env>/<account>.json`
holds `ASP.NET_SessionId`, `.ASPXAUTH`, `ehrm85` (and local storage). For QA on
2026-10-10: `automation.json` (saved 15:52), `evelyn.json` (2026-10-09 20:16),
`evelyn1.json` (15:58). Likely ways the pair stops matching:

1. **The server moved on.** PeoplesHR allows one session per user; if the
   account signed in elsewhere after the session was saved, the server's
   session/user context no longer matches the restored cookies - which fits
   "sometimes".
2. **Account switch in one browser.** A `sign_in` action mid-case replaces
   `.ASPXAUTH`, but `ehrm85` may still carry the previous user until a page
   rewrites it.
3. **A session saved mid sign-in.** Captured after `.ASPXAUTH` changed but
   before `ehrm85` was rewritten - then every restore fails until it is saved
   again.

**To pin it down.** When it appears, note the account, which browser (run,
discovery, supervised) and what came just before (a `sign_in` step, a sign-in
elsewhere with the same account). A safe test: delete the files in
`autorun/sessions/<env>/` so runs sign in fresh and save new sessions; if the
error stops, a stale saved session was the cause.

**Fix in the app.**
1. Recognise this error page (and the empty 400) as "session no longer
   valid": drop that account's saved session and sign in fresh, once, instead
   of failing the case.
2. Save a session only after confirming the page shows the expected user
   (e.g. the user name/employee number in the header matches the account).
3. On a mid-case `sign_in`, clear the site's cookies before signing in the
   other account, so no cookie of the previous user survives.
