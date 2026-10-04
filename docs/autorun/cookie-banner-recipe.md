# Dismissing PeoplesHR's cookie banner from the sign-in recipe

Written 2026-10-05 for the PeoplesHR project (`peopleshr__hrm-b9614466`).

## The problem

PeoplesHR shows a "Cookie Consent" bar fixed to the bottom of every page until it
is dismissed. It sits over whatever is at the bottom of the window. In the
Participants run on 2026-10-04, case 137574 failed because the bar covered the
Employee Search iframe's Submit button: a search listing 25 rows pushed Submit
down under it.

```
button "Submit" in Iframe "Employee Search" in dialog "Search Employees"
is covered by div#cookieBanner.cookie-banner.show
```

Any case whose control lands near the bottom of the window can hit this.

## How the banner works

Read from the sandbox's own `/resources/js/Version5/cookie-consent.js` on
2026-10-05:

| Control | What it does | Lasts |
|---|---|---|
| `#btnCookieClose` (the ×) | `DismissBanner()`: sets a flag in the tab's `sessionStorage` and hides the bar. Nothing is sent to the server. | That browser tab, across page loads, until the tab closes |
| `#btnCookieAcceptAll` ("Accept All") | `AcceptAll()`: signed out, posts `api/securityapi/SaveAnonymousCookieConsent`, which sets the HttpOnly cookie `ehrm_cookie_consent` for 365 days; signed in, posts `api/securityapi/SaveCookieConsent`, which stores the consent against the account on the server | 365 days, or until the policy version changes |

The bar's own id is `#cookieBanner`; it has the class `show` while visible.

Note: the script never sets its `SESSION_DISMISSED_KEY`, so the flag is stored
under the key `"null"`. Dismissing still works, because the same `null` key is
read back.

## Why the current recipe misses it

The recipe already clicks the × - but in its sign-in `steps`, on the login page:

```json
{ "kind": "when_visible", "selector": { "css": "#btnCookieClose" }, "within_ms": 3000,
  "then": [ { "kind": "click", "selector": { "css": "#btnCookieClose" } } ] }
```

Those steps only run when the app signs in afresh. When a case reuses the
account's saved session, the login page is never shown, the × is never
clicked, and the bar stays up for the whole case. The recipe's
`after_sign_in` list runs after BOTH kinds of sign-in, before the module path.

## The change

Add the same `when_visible` to the start of the recipe's `after_sign_in`, so
it runs after every sign-in, saved session or not:

```json
"after_sign_in": [
  {
    "kind": "when_visible",
    "selector": { "css": "#btnCookieClose" },
    "within_ms": 4000,
    "then": [ { "kind": "click", "selector": { "css": "#btnCookieClose" } } ]
  },
  { "kind": "when_visible", "selector": { "css": ".bootbox.modal.show .modal-footer button" }, "within_ms": 4000,
    "then": [ { "kind": "click", "selector": { "css": ".bootbox.modal.show .modal-footer button" } } ] },
  { "kind": "when_visible", "selector": { "css": "#sidebar-toggle-menu:not(.active)" }, "within_ms": 5000,
    "then": [ { "kind": "click", "selector": { "css": "#sidebar-toggle-menu" } } ] }
]
```

- **Where:** the project's sign-in recipe, in the app (Auto Run, sign-in recipe), or the file
  `%APPDATA%\com.avinalwis.testcasemanager.v2\autorun\projects\peopleshr__hrm-b9614466.json`
  with the app closed. Keep the existing two `after_sign_in` entries as they are.
- **Why first:** the bar can sit over the session prompt's OK button and the menu toggle.
- **Why 4000 ms:** the bar is shown by a `setTimeout` after the page scripts start; the home
  page can be slow (the module-path stall seen on first cases), and `when_visible` costs nothing
  when the bar never appears.
- **API templates** sign in through the same recipe, so their browser gets the same dismissal.

## Why the ×, not Accept All

The × changes nothing outside the run's own browser tab. Accept All records a
consent: a year-long cookie, or a consent stored against the shared test account
on the server. That is a decision for the person who owns the account, not
something a test run should make on their behalf. If you decide to accept for
the test accounts once, do it by hand in the supervised browser while signed in
as each account; the banner then stops appearing for those accounts until the
policy version changes, and the recipe entry above becomes a no-op.

## Checking it worked

1. Re-run 137574 unattended.
2. In Settings, Logs, the sign-in line for the case should list `#btnCookieClose` among the
   `after_sign_in` selectors that appeared, also when it says "from a saved session".
3. The case passes its step 4 (Submit clicked, the 3 employees in the grid).

Nothing in any script changes. Scripts must not click the banner themselves:
an unconditional click fails whenever the bar is not shown.
