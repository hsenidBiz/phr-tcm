# Definition Wizard automation: tokens per test case, TCM Auto Run vs Playwright

Measured 2026-10-13 from the Claude Code transcripts of the two sessions that
automated the PMS Definition Wizard (PBI 133443): every assistant message's
`usage`, counted once per message id, in the main session and in every
subagent and workflow transcript of that session.

| | Playwright repo | TCM Auto Run |
|---|---|---|
| Session | "Avin - Playwright Automation" (`D:\PHR-PLAYWRIGHT-AUTOMATION`) | "Avin - TCM Auto Run Scripts" (this repo) |
| Window | 2026-10-06 11:47 → 2026-10-09 12:15 | 2026-10-05 08:52 → 2026-10-08 08:15, less the hour of app code changes on 10-05 (09:41-10:40) |
| What it produced | 75 tests in 6 page specs (`suites/sl/admin/definition-wizard/*/specs/*.spec.ts`), one `// ADO <id>` case each; no Preview spec | 135 Auto Run scripts, one case each, 7 pages incl. Preview |
| Agents (subagent/workflow transcripts) | ~480 | 29 |

## Totals

| | Playwright | TCM |
|---|---|---|
| Input tokens (fresh + cache writes + cache reads) | 662 M | 321 M |
| — cache reads | 627 M | 313 M |
| — cache writes | 34.6 M | 8.2 M |
| Output tokens | 691 k | 224 k |
| Price-weighted* | ≈ 109 M | ≈ 43 M |

## Per test case

| | Playwright (75) | TCM (135) | Playwright ÷ TCM |
|---|---|---|---|
| Input tokens, all kinds | ≈ 8.8 M | ≈ 2.4 M | ≈ 3.7× |
| — cache reads | ≈ 8.4 M | ≈ 2.3 M | |
| — cache writes | ≈ 460 k | ≈ 61 k | ≈ 7.5× |
| Output tokens | ≈ 9.2 k | ≈ 1.7 k | ≈ 5.5× |
| Price-weighted* | ≈ 1.46 M | ≈ 0.32 M | ≈ 4.6× |

\* A single cost-like figure, weighting each kind by its relative price:
cache read × 0.1, fresh input × 1, cache write × 1.25, output × 5. Token
counts only - not per-model price.

## Why TCM uses fewer

- **Runs cost no tokens.** The app executes Auto Run runs itself; an agent
  only reads failures. In Playwright the agents run, heal and refactor the
  specs - most of 2026-10-08 and 10-09 (365 transcripts on 10-08 alone).
- **Fewer, longer-lived agents.** 29 vs ~480; each new agent re-reads its
  context, which shows up as cache writes (7.5× per case).
- **No code to write.** A TCM script is JSON actions saved through a checked
  tool; Playwright generates `.page.ts` and `.spec.ts` files and then
  refactors them.

## Caveats

- Each window also holds other work: TCM includes correcting test cases and
  setting up data with API templates; Playwright includes running, healing and
  refactoring. The windows are cut by date and topic, not per test.
- Per-case figures are averages. A per-page split is not reliable: the
  Playwright workflow agents' prompts do not name a page, so almost all of its
  tokens cannot be tied to one.
- Model mix is not counted: much of TCM's script work ran on Sonnet, which
  costs less per token.
- Pass rates are not compared here.

## How it was measured

Per transcript (`~/.claude/projects/<project>/<session>.jsonl` and its
`subagents/` and `workflows/` folders): sum `input_tokens`,
`cache_creation_input_tokens`, `cache_read_input_tokens` and `output_tokens`
of each assistant message, once per `message.id`. Main-session messages are
counted inside the window; a subagent or workflow transcript is counted whole
when its first message falls inside the window.
