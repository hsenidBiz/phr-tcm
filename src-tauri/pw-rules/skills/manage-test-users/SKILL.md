---
name: manage-test-users
description: Use when adding, inspecting, editing, renaming or removing a test account in src/users/users.json - "add a payroll supervisor", "change this account's password", "which specs use Michael", "remove that account", "rename this user key". The only sanctioned way to change the account registry; never hand-edit users.json.
---

# Managing test accounts

`src/users/users.json` is the registry every spec declares an account from.
This skill gathers what a human knows and hands it to `npm run users`, which
decides what is valid.

**Never edit `users.json` directly.** Not as a fallback, not when the CLI
refuses. A refusal is the tool working: it has found a reason the change would
break something, and that reason is in the message.

**The same add/edit/delete/rename are also in `npm run qa`'s account screen**
(`F6`/`u` from the test browser, then `a`/`e`/`d`/`n`, `p` to reveal a masked
password). It calls the same `cli/users/actions.mjs` the CLI calls, so a
refusal reads identically either way — this skill does not have to pick one
over the other for correctness. Reach for the console when a human is
sitting at the keyboard for a one-off change and wants the spec counts
in front of them while deciding, e.g. spotting `unused` before deleting, or
seeing a supervisor's count climb past what one account should carry. Reach
for the CLI — which is what this skill drives — for anything scripted or
run unattended, and for the five things the console does not do:

- `import` (bulk-seeding from a spreadsheet)
- `check`
- `list --json` (what the pipeline itself calls during test generation)
- `add --supervisor` — the console's `add` asks for the key, username,
  password and the three identity fields, and **cannot set a supervisor
  pointer at all**. Reach for the CLI whenever the account has one.
- `rm --dry-run` — the console's `d` plans and refuses in one step, so
  there is no way to list an account's references without attempting the
  delete.

That missing `--supervisor` is worth one more sentence, because it costs
more than a field. `cli/users/references.mjs` records that a computed
`actingAs(user.supervisor.key)` call site is invisible to the reference
scan, and that the registry's own `supervisor` pointer is **the only
backstop** that keeps `rm` from orphaning it. An account created through
the console carries no such pointer, so for that account the backstop is
simply absent. Add supervised accounts through the CLI — or, on an account
that already exists, set the pointer afterwards with
`npm run users -- set <key> supervisor <supervisor-key> --apply`.

This is **not** the skill for `suites/**/data/<feature>.json` - that is
`test-data`. Different files, different rules: data files carry `_provenance`
and `{{unique}}` tokens and are per feature; the registry is global and has
referential integrity they do not.

## Before anything else

Run `npm run users -- list` and show it. Every request below is easier to get
right when both of you can see what already exists.

## Adding an account

Ask for whatever is missing, one thing at a time:

- **the key** - `<environment>.<module>.<role>`. Environment matches the
  existing entries' casing (`AutomationSL`). Module is a label, not a path.
  Role must be in `KNOWN_ROLES` (`general`, `supervisor` today); adding a new
  role is a one-line edit to `cli/users/validate.mjs` and should be a
  deliberate decision, not a side effect of a typo.
- **username and password** - both required.
- **identity fields** - `--employee-no`, `--display-name`, `--emp-title`.
  Optional, and genuinely absent for service accounts. Do not invent them: a
  fabricated `displayName` fails a test confusingly instead of failing at
  declaration time.
- **supervisor** - the *key* of another account, never a name.

Preview first, then apply:

    npm run users -- add AutomationSL.payroll.supervisor --username Ava --password '...'
    npm run users -- add AutomationSL.payroll.supervisor --username Ava --password '...' --apply

**If it refuses with a discriminator collision**, it means an account for that
module+role already exists without a discriminator. Both need one. The message
names the rename to run first; show it to the human and confirm before running
it, because renaming touches every spec that declares the old key.

## Changing a field on an existing account

Rotating a password, correcting a username, filling in an identity field or
setting a supervisor pointer — all of it is one command:

    npm run users -- set <key> <field> <value>
    npm run users -- set <key> <field> <value> --apply

Fields: `username`, `password`, `employeeNo`, `displayName`, `empTitle`,
`supervisor`. A value of `-` clears an optional one (the four after the
credentials); `username` and `password` cannot be blanked. A multi-word value
does not need quoting. In the console it is `e` on the highlighted account:
pick the field by number, type the value.

**This does not care how many specs use the account.** Unlike `rm`, which
refuses on any reference, a heavily-used account is exactly the one whose
password you need to rotate. Nothing needs rewriting either: a spec names only
the *key*, and reads `user.username`, `user.displayName` and the rest through
the `user` fixture at run time.

**Changing `username` or `password` deletes the account's cached `.auth`
session, and that is the point.** The cache is named by key shape and trusted
on mtime alone for eight hours, so a credential change that left it in place
would keep every run signed in with the *old* credentials — and because the
server goes on honouring the existing cookie, nothing would surface the
mismatch. The new password would simply go untested until the TTL expired.
The four identity fields cannot affect a login, so they leave the cache alone
and cost no re-login.

To change the *key* itself, use `rename` — not this. That one does carry a
cascade, because the key is the part that appears in spec source.

## Removing an account

Always look first:

    npm run users -- rm <key> --dry-run

This lists all four places a key can be named: a spec's
`test.use({ userKey })`, a spec's `actingAs('<key>')` call, a test case's
`**User:**` line, and another account's `supervisor`. If anything is listed,
removal is refused - and rightly: the spec would fail at run time with an
error that never mentions the registry. Deal with the references first.

**One shape it cannot see:** `actingAs(user.supervisor.key)` and any other
computed argument, which needs the registry at run time to resolve. Those are
not reported rather than guessed at. The `supervisor` pointer covers the case
that actually occurs, but if you know a spec computes a key, check it by hand.

On success `rm --apply` also deletes that account's cached session under
`.auth/`, so the same key added back the same day cannot inherit the deleted
account's login.

## Renaming an account

`npm run users -- rename <old> <new> --apply` moves the entry and repoints
supervisors, but refuses while a spec or test case still names the old key -
whether through `test.use({ userKey })`, `actingAs('<key>')`, or a test
case's `**User:**` line - and the refusal now names the flag that unblocks
it.

`--cascade` is that flag. It rewrites the four **structural** usages in
place: a spec's `test.use({ userKey: 'K' })`, an `actingAs('K')` call, a test
case's `**User:** K` line, and another account's `supervisor` pointer (that
last one inside the registry object `rename` already writes, not by editing
text). Those four have syntax that means "this runs as K" and nothing else,
so rewriting them is safe.

    npm run users -- rename AutomationSL.payroll.general AutomationSL.payroll.general2 --cascade
    npm run users -- rename AutomationSL.payroll.general AutomationSL.payroll.general2 --cascade --apply

What it does **not** touch: prose that merely *mentions* the key - the CLI's
own help text, this skill's example commands, `gen-test.md`'s prose, a
scratch registry inside `cli/users/*.test.mjs` that just happens to look like
a key. Grepping for a key finds all four structural forms plus these; only
the structural ones have a shape a program can rewrite without guessing. A
cascade that rewrote every occurrence would corrupt a test fixture that is
*supposed* to contain the string - worse than the refusal it replaces. Those
mentions are reported by file and line instead, and the report says plainly
they need a human's eye before the rename is really done. Both surfaces print
that report - it comes back on the plan, not from either front end - and it
includes the stale occurrences sitting *inside* a file the cascade rewrote:
a comment saying which key the spec used to declare is exactly the kind of
thing that outlives the rename. What is never scanned: `node_modules/`,
`.git/`, `reports/` and `test-results/` (run output, which no rename can
invalidate), `docs/superpowers/plans/` (the historical planning record, which
is deliberately never edited) and `.claude/worktrees/` (another branch's
checkout of this same repo - its files are not yours to review).

The provenance marker on a `**User:**` line - `<!-- assumed -->` or
`<!-- from-ticket -->` - is left untouched by the cascade. That marker
records how the account was *chosen*; a rename only changes how the key is
*spelled*, so there is nothing for the cascade to update there.

The console's rename (`n` on the account screen) always plans with the
cascade - there is no non-cascading rename there - and folds the edit list
and the mention report into its `y`/`n` confirmation prompt. Declining writes
nothing. The plain CLI `rename` without `--cascade` is the only place a
rename can still refuse on a structural reference alone.

Like `rm`, `rename --apply` deletes the OLD key's cached session under
`.auth/`. The cache is named by key shape, so leaving it there would let the
old key - re-minted later for somebody else - inherit the renamed account's
login for the rest of the eight-hour TTL. The renamed account logs in fresh
on its next run.

## When something is wrong

`npm run users -- check` runs every registry rule and names what failed. Run it
after any hand edit you find in the wild - and then ask whoever made it to use
this skill instead.

## One rule that is not about this file

An admin spec and a self-side spec must not share an account. Which side an
account is on is server-side state that persists across logins, so a spec that
switches to self changes where every admin spec using that account starts. If
someone is adding an account because two specs are fighting, that is usually
why.
