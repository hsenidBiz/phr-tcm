# Unattended runs wait out a dropped VPN

Date: 2026-10-14. Status: design, written from the design conversation; awaiting
Avin's review.

## Why

Avin works remotely through FortiClient. When the VPN drops during an unattended
Auto Run, the cases that follow fail with network errors: a case's setup reads and
writes the environment's database, and that database is only reachable on the
company network. The run ends with a list of failures that say nothing about the
cases themselves, and the run has to be redone by hand.

A built-in VPN client was tried first (OpenConnect with Fortinet single sign-on) and
proved on Avin's machine to sign in and open a tunnel, but the gateway ends every
non-FortiClient tunnel ten seconds after it opens. TCM therefore cannot reconnect the
VPN itself. What it can do is notice that the company network has gone, stop
spending cases while it is gone, tell Avin, and carry on by itself once FortiClient
is reconnected.

## Goals

- An unattended run never records a case as Failed or Blocked because the company
  network was unreachable.
- When the network goes, the run pauses, Avin is told (a Windows notification when
  TCM is not in front, plus the run's own pause view), and the AI assistant can see
  the same state.
- When the network comes back, the run resumes by itself, with no click.
- In the office, or with FortiClient connected, nothing about runs changes.

## Non-goals

- Connecting, reconnecting or controlling any VPN. TCM never starts FortiClient,
  stores VPN settings or signs anyone in.
- Supervised runs, replay-to-step, discovery and the assistant's own tools. Only the
  unattended run (`auto_run_replay`) pauses.
- Resuming inside a case. A case interrupted by the drop is run again from its setup.
- A setting for the wait limit (fixed at 30 minutes, see below).

## Anti-goals (a failure even if it technically works)

- A real failure hidden as a network pause: a case whose setup or steps fail while
  the database server answers is recorded exactly as today.
- A run that waits forever: the pause gives up after 30 minutes.
- Any database login, query or password used to decide reachability.
- A host, address or password in an event, a run file, a notification or an
  assistant answer (the existing rule for `AutorunResetNeeded`).

## What "the company network" means

The active environment's database server. The environment names its database
(`Environment.db_id`); that database's server and port (default 1433) are already
known to TCM. **Reachable** means a plain TCP connection to that server and port
succeeds within 3 seconds - no login, no query, no `sqlcmd`.

An environment with no database, or whose database cannot be resolved, has no
reachability check: runs behave exactly as today.

The QA site itself is reachable from the internet, so it is not the signal.

## Behaviour

**Gate.** Only when Auto Run is offered (`ai_tools::autorun_offered()`: Extras or
Advanced Features on, or a development build) - the unattended run is already behind
that gate, so nothing new is gated separately.

**Armed once the database has answered (Avin, after the final review).** Every
environment names a database (a new one gets the default), so "no database, no check"
almost never applies. A run therefore only starts watching once the database server has
answered at least once in that run: until then every check counts as "no check", and a
run started off the VPN behaves exactly as today. This is the "it was there and went
away" case the feature is for.

**Before each case** (including the first): check the database server. Reachable: run
the case as today (and the run is armed). Unreachable: pause before the case - only once
armed.

**After a case ends Failed or Blocked:** check the database server. Reachable: keep the
record as today (including the existing one-time transient retry). Unreachable: the
record is thrown away, the run pauses, and after the pause the same case runs again
from its setup, as if for the first time. The case's existing transient retry still
applies to that new go.

**The pause.**
- The run's progress shows a waiting state: "Paused - the company network is
  unreachable. Reconnect the VPN; the run carries on by itself." with how long it has
  waited and when it will give up, and a Stop button. The title-bar run pill shows
  the same in the warning tone (as the existing "Paused before Batch N" does).
- A notification is raised once per pause: a toast when TCM is in front, a Windows
  notification when it is not (the existing `announce` helper).
- The database server is checked every 5 seconds. The moment it answers, the run
  resumes, with no click, and the waiting state clears.
- Stop ends the run: the remaining cases are recorded as not run, with the reason
  "the run was stopped while the company network was unreachable".
- After 30 minutes without the network, the run ends the same way with the reason
  "the company network was unreachable for 30 minutes", and a second notification
  says the run gave up.
- Closing the app during a pause ends the run as a Stop (as the existing pause does).
- The pause is recorded on the run (when it began, how long it lasted, before which
  case, and whether it resumed, was stopped or gave up), so the review and Past runs
  can show "waited 4 min for the network before case #135520".

**The assistant.** The assistant's existing view of an unattended run says when the
run is paused for the network and since when. Read only: the assistant cannot answer
the pause - only the network coming back, Stop or the time limit end it.

## Decided in the conversation

- The signal is the environment's database, because that is what the failing runs
  needed and the site is public.
- Wait until the network is back, with Stop available, and give up after 30 minutes
  (Avin accepted 30 minutes).
- Notify Avin when the run pauses.
- Only for Avin for now, only when Extras are enabled.

## My calls (Claude), for review

- A case is treated as a network casualty only when the database server does not
  answer *at the moment it ends*. A failure while the server answers is a real result.
- The interrupted case runs again from its setup rather than being marked; the case
  before it is never re-run.
- 3-second connect timeout, 5-second recheck interval, 30-minute limit: constants,
  not settings.
- Uses the existing pause pattern (one waiting slot, cancel-aware, ended on app exit)
  with its own event and record rather than overloading the batch reset pause.
- No new settings card: the behaviour is automatic whenever Auto Run is offered and
  the environment has a database.
- **The limit is per pause; a case is re-run after a network pause at most 3 times.**
  A flapping VPN could otherwise re-run one case endlessly. After the third re-run
  the case's record is kept as it ended, with "(the company network was unreachable
  each time this case ran)" appended to its reason, and the run moves on.
- **Resolving the server needs no login and no `sqlcmd`:** the server and port come
  from the database's saved details (`DbDatabase.server` / `port`, default 1433),
  so the check works even when no login is saved or `sqlcmd` is missing, and
  regardless of the Database Read Access switch (setup and fixtures still use the
  database). A `tcp:` prefix and a `host,port` form are understood. A named instance
  with no port (`host\instance`) has no fixed port to try, so it gets **no check**:
  a false "unreachable" (a healthy run paused for 30 minutes, then ended) is worse
  than no protection.
- **A drop that heals before the case ends is not caught.** The check is "unreachable
  when the case ends"; a few-second blip that is over by then leaves that case's
  failure as recorded. Matching failure text to guess at network errors was rejected
  (the transient-retry module's rule: judged by where a failure sits, never by its
  words alone). Full disconnects - the case that motivated this - are caught.
- **The after-case check sits after the case's final record,** including the
  existing transient retry: the retry may spend one more go during an outage, which
  is harmless.
- **A discarded go is re-run the way the transient retry re-runs one:** setup runs
  again first (`setup::prepare_case`), so drafts and fixtures are prepared afresh.
  Its screenshots are left for the existing screenshot pruning.
- **The assistant's view:** a small read-only addition to the assistant's tools that
  says whether an unattended run is waiting for the network, since when, before which
  case, and when it will give up - no host, no address.

## Left to the builder

- Where exactly in `replay::run_cases_planned` the checks sit, and how the pause is
  threaded through (a gate alongside `ResetGate` is the expected shape).
- The event, record and command names, and how the existing pause UI is reused or
  extended for the network wait.
- How the assistant's run view exposes the waiting state.
- How the database server is resolved from `db_id` (the way
  `preconditions::environment_db` does it is the model).

## Testing

- The decision rules with a fake reachability check: reachable before and after
  (unchanged); unreachable before a case (pause, then the case runs); a failed case
  with the server unreachable (record dropped, pause, case re-run); a failed case with
  the server reachable (record kept); no database configured (never checks).
- The pause: resumes when the check turns reachable; Stop records the rest as not
  run; the 30-minute limit (with an injectable clock) gives up with its reason; app
  exit ends it.
- Nothing about the run's records, events or notifications names a host or address.
- The UI: the waiting state, its Stop, the pill, and the notification raised once.
