# API templates: reuse a signed-in browser across runs

Date: 2026-10-09
Status: design approved in conversation (backlog item 17).

## Goal

Back-to-back single API template runs (`run_api_template`, `prove_api_template`) on the same environment and account skip the browser launch and the sign-in, taking about a second instead of 10 to 30 s. Nothing a run checks or records changes.

## Owner decisions

| Question | Decision |
|---|---|
| Idle window | 2 minutes after a run ends; each reuse restarts it |
| Dropped session | An empty 400 or a sign-in page closes the held browser; the run signs in fresh once and carries on |
| Sharing an account | The held browser gives way to anything else that needs the account |

## 1. The held browser

- After a single template run ends, whatever its outcome, its browser is kept open and signed in instead of closed, keyed by (environment id, account key). At most one is held per key. A newer one replaces an older one, and the older one is closed.
- It is kept only if the run signed in successfully and the browser is still alive.
- It is closed when:
  - 2 minutes pass with no run using it (`HELD_IDLE = 120 s`);
  - the app quits;
  - the active environment changes;
  - the person signs out;
  - the run's sign-in recipe or account changes since it signed in (compared by a fingerprint of the recipe and the account's login, never logged).
- Fixtures are unchanged: they keep their own browser for their own steps and close it at the end.

## 2. Reusing it

- A run takes the account's lease exactly as today, for its whole run.
- With the lease held, the run looks for a held browser for its key.
  - **If one exists and is still valid** (same fingerprint, not given way, still alive), the run reuses its session and skips the sign-in.
  - **Otherwise** it opens a browser and signs in as today.
- **The token page.** If the browser is already on the template's anti-forgery page (same path), the run skips the navigation. It still re-reads the token from the page, which is cheap. If the page differs, it navigates as today.
- **A dropped session.** The first request comes back as an empty 400, or the browser lands on the sign-in page. The held browser is closed, a new browser opens, signs in once and runs the template from the start. This happens once per run and only for a reused browser. A fresh sign-in that hits the same thing fails as today.
- The run report says whether the sign-in was reused ("Signed in earlier, reused"), so a failure can be read.

## 3. Giving way

- An idle held browser holds **no** lease.
- Whenever any other holder takes the lease for that (environment, account), the held browser for that key is marked as given way. Other holders include an unattended case, the supervised Auto Run browser, Auto Run setup, a fixture or a discovery. A template run does not count, since it is the one that would reuse it.
- A held browser that has given way is never reused. It is closed the next time the held store is touched, or when its idle time runs out.
- A person signing in elsewhere, outside the app, cannot be seen. The dropped-session rule in section 2 covers it.

## 4. Errors and limits

- A held browser is closed on every error path. The idle timer never panics; a close that fails is logged and forgotten.
- No secrets, logins, hosts or query strings in logs. Log lines name the account key only: "kept signed in as <key>", "reused", "given way", "closed after 2 minutes idle".
- No HTTP DELETE to Azure DevOps.
- The machine is shared: the held browser is one Edge process at most per (environment, account).

## 5. Testing

- **Rust**, with the fake `Browsers`/`Driver`:
  - a second run within the window opens no browser and does no sign-in;
  - after the window ends, the browser is closed;
  - a reuse restarts the window;
  - a changed recipe or account is not reused;
  - a lease taken by a case or the supervised browser makes the held browser give way;
  - the token page navigation is skipped on the same page and taken on a different one;
  - an empty 400 on a reused browser leads to one fresh sign-in and the run carrying on;
  - the same on a fresh browser fails as today;
  - a failed sign-in keeps nothing;
  - quitting the app, changing environment and signing out each close it.
- **Hand check owed:** two real template runs within 2 minutes; the second skips the sign-in.
