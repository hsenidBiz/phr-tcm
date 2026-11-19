# Fewer sign-ins in an Auto Test run - design

Date: 2026-11-19. Owner chose "both" (A and A2) after the measurement below. Status: short design, the owner's calls as decided, the rest marked as my calls.

## Why

104 saved runs: signing in again for every case took 1.7 h of 5.1 h (median 5.2 s per case). All 784 consecutive case pairs used the same account, and 416 of 418 timed cases start with a trip to their module. No saved run used a setup.

## What changes

- **A2 - keep the signed-in browser between cases (the saving).** After a case ends, its browser stays open, signed in, when the next case uses the same account. The next case skips opening a browser and signing in: it checks the browser is still signed in (`signin::already_signed_in_why`, the existing 2 s look) and goes straight to its module trip (`nav::TripFrom::Elsewhere`, which already handles "a browser already in use").
- **A - a setup's browser is the kept browser too.** A case with a setup runs the setup's templates in the kept browser when the setup's account is the one it is signed in as, instead of opening and closing a browser of its own. A different account closes the kept browser first. There is never more than one signed-in browser per run (some applications refuse a second session for the same account).

## When a fresh browser is used instead (my calls, on the side of today's behaviour)

- The account changes, or the case has no module trip (it would start wherever the last case left off).
- The case before failed, was Blocked, was stopped, or failed to sign in. A script with its own `sign_in` step also counts, because it may have changed the account.
- The case before ran with the no-save guard on, since the guard cannot be taken off a page.
- The still-signed-in check fails. That browser is closed and the case signs in as today.
- Around a batch pause or a network pause, the kept browser closes first, so the person changes things with no browser open, as today.
- A transient retry always uses a fresh browser, as today.
- **A case that fails in a kept browser runs once more in a fresh one, and the fresh run is the record.** Leftovers in the browser (stored filters, client-side state) then cannot produce a failure that a fresh browser would not have. The case's video comes from that fresh run.

## Unchanged

- The account lease is held while the browser is kept and let go when it closes.
- Every tab but the main one is still closed after each case.
- Downloads stay in the run's folder.
- Each case keeps its own record. Step 0 says the case "used the browser the case before signed in" with the check's time, and the phases show open and sign-in near 0.
- No new setting (lean controls). Turning reuse off would only bring back the old cost.

## Tests (Rust, `tests/suite/`)

- Same account, both with module trips: one browser opens for two cases and one sign-in happens.
- An account change, a failed case before, a no-save case before, a case with no module trip, and a pause in between each give a fresh browser.
- The still-signed-in check failing gives a fresh browser and a normal sign-in.
- A case failing in a kept browser runs again fresh, and that run is the record.
- A setup on the same account runs in the kept browser; a setup on another account closes it first.
- Stop and the run's end close the kept browser and let go of its lease.
