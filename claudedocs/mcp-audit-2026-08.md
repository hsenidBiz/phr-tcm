# MCP & Azure DevOps REST Audit — August 2026

**What this is.** A differential audit of Test Case Manager V2's MCP server
and its Azure DevOps REST usage, against the MCP specification, Microsoft's
own `microsoft/azure-devops-mcp` server, and the Azure DevOps REST
documentation. Everything below is evidence-backed and was adversarially
re-verified: a second pass tried to *refute* each finding before it was
allowed into this report.

**Reference snapshot:** `microsoft/azure-devops-mcp` @
`f165b954cadb525374fa4bfbd48f3285f9f783c5`
**MCP spec:** judged against revision **2025-11-25** (see A-1 for why, not
the current 2026-07-28)
**Audit date:** 2026-08-25 · **Rows:** 92 · **Verified:** 91 · **Downgraded:**
1 · **Dropped:** 0 · **Citations corrected during verification:** 24

---

## Summary

**Protocol lane — sound, with one real defect.** The headline result is
positive and was the audit's biggest unknown: our MCP layer is hand-rolled
(Microsoft's gets conformance free from the official SDK), and a
**spec-conformant third-party client interoperates with it end to end** —
Microsoft's own MCP SDK client completed the full lifecycle against our real
`v2.exe --mcp`, listed all 15 tools with well-formed schemas, and executed a
live tool call through the bridge, with no hangs and no state corruption. One
genuine defect (P-8) and two divergences worth fixing sit on top of that.

**REST lane — no broken calls.** Across 78 rows covering every
bridge-reachable Azure DevOps call, **nothing is broken**: URLs, api-versions
(7.1 GA throughout), parameters, hosts and paging all match the documented
contracts. Seven divergences are worth attention, three of them likely to bite
in normal use. Notably, the `System.Tags` add-vs-replace fix shipped earlier
today is **confirmed correct** by ADO's own documentation.

**Where the ecosystem is.** Neither our server nor Microsoft's implements the
current MCP revision (2026-07-28), which replaces the `initialize` handshake
with per-request metadata. Both sit on the legacy handshake era, which the
spec explicitly permits modern clients to keep serving. We are not behind the
reference — the whole ecosystem is mid-migration.

---

## Findings register

Severity: **broken** (wrong behavior) · **risky divergence** (works today,
fails on a reachable input) · **benign divergence** (different, defensibly) ·
**info** (context, no action implied).

### Broken

**P-8 · An unknown tool name reports a dead bridge.**
`mcp.rs:437-461` funnels *every* `Err` — including "no such tool" — through the
bridge-unreachable formatter, so a typo'd tool name answers:
`"Could not reach Test Case Manager (unknown tool foo_bar). Start the app and
sign in, then retry."` An assistant reading that goes hunting for a dead
bridge instead of correcting its tool name, and the write-refusal path has the
same problem: a write-smelling unknown name returns WRITE_REFUSAL wrapped in
the same misleading sentence. The MCP Tools spec's §Error Handling lists
"Unknown tools" under **Protocol Errors** with the worked example
`{"code": -32602, "message": "Unknown tool: invalid_tool_name"}`.
*Both audit lanes found this independently* — the static read and the live
smoke, which recorded the identical string in bridge-up **and** bridge-down
runs (killing the "unreachable path" refutation). No test covers it.
Related, same row: declared `required` fields are never validated
(`mcp.rs:307` reads `args["pbi_id"].as_i64()` and proceeds regardless).

### Risky divergences

**R-1 · Retry-After is honored only on 429; Azure DevOps throttles on 200
first.** *(highest real-world impact)*
`ado/transport.rs` reads `Retry-After` solely inside its 429 branch
(`retry_after()`, consumed at :125/:143/:260). Microsoft's rate-limit guidance
says, verbatim: *"Honor the Retry-After header: If you receive it in a
response, wait the specified time before sending another request. **The
response still returns HTTP 200**, so retry logic isn't required."* Delays
range "up to 30 seconds". So during throttling the app keeps firing at its
fixed 200 ms pace while the server is asking it to slow down — pushing the
user toward the hard 429 the pacer exists to avoid. A full re-read of
transport.rs plus a repo-wide grep confirmed only two header reads exist in
the entire funnel, one of them 429-gated.

**R-2 · Wiki search hands back a file path; the page fetch wants a page
path.** `search_wiki` surfaces the search result's `path` (`endpoints.rs:156`)
— which lives in ADO's **gitItemPath** namespace (`/Hello-world.md`, per
Microsoft's own sample) — and `get_wiki_page` splices it straight into the
Pages-Get `path=` parameter (`endpoints.rs:175-180`), documented as *"Wiki page
path"*. The two are distinct fields in ADO's own type definitions. Because ADO
also converts spaces to hyphens in git file names, **no string fix-up recovers
the page path** for titles containing spaces. Verification surfaced
corroborating evidence the original audit missed: our own wiremock fixture
(`tests/ado.rs:137-159`) encodes the `.md` shape. *One live call would settle
whether Pages-Get 404s or tolerates the extension — until then this stays
"risky", not "broken".*

**R-3 · The `TestedBy` relation filter is direction-blind.**
`endpoints.rs:710-715` (duplicated at :278-284) matches relations with
`contains("testedby")`, which matches **both** link directions; Microsoft's
server compares `relation.rel === linkTypeName` against two distinct constants.
The only validation on the bridge path is an `i32` parse (`ai_bridge.rs:1400`),
so passing a Test Case id as `?pbi=` follows the reverse link and returns the
PBI rendered as a test case. *(Verification corrected the original row: the
GUI-editor escalation it claimed does not hold; the bridge path does.)*

**R-4 · Protocol version is echoed, never negotiated.**
`mcp.rs:39-43` returns whatever the client sent (defaulting to `2024-11-05`),
with no supported-version list anywhere. The Lifecycle spec: *"If the server
supports the requested protocol version, it MUST respond with the same
version. Otherwise, the server MUST respond with another protocol version it
supports."* Echoing `2026-07-28` would assert support for a dialect we do not
implement. No code branches on the negotiated version today, so nothing
downstream is wrong yet — hence risky, not broken. (The live smoke negotiated
`2025-11-25` correctly *by coincidence*: it echoed the SDK's own request.)

**R-5 · Continuation tokens are appended raw, in unbounded loops.**
`ado_testplan/plans.rs:19-21, :71-73, :111-113` and `runs.rs:38-40` do
`url.push_str(&format!("&continuationToken={c}"))`. The `url` crate's query
percent-encode set does **not** cover `&`, `+` or `%`, so a token containing
one would corrupt the query; the loops have no cycle guard or page cap, so
the failure mode is an infinite loop rather than an error. Microsoft's server
percent-encodes via `URLSearchParams`. *Whether ADO's tokens can contain those
characters is undocumented — the trigger is unresolved, the exposure is not.*

**R-6 · Org/project names are interpolated into URLs unencoded.**
`endpoints.rs:66-69, :214-217, :362-365, :943-946` interpolate raw, while
`board.rs:252` encodes the same value. Verification found this *more*
reachable than first claimed: `%` is legal in ADO project names and is not
UI-restricted, so a project like `"50% Done"` produces an invalid escape
sequence on the wire.

**R-7 · A bridge timeout is reported as "start the app".**
`mcp.rs:480` sets a 30 s timeout whose expiry emits the same "Start the app
and sign in" message. Since ADO's documented throttle delays reach 30 s, a
throttled-but-healthy app can be reported as not running.

### Benign divergences and info (selected)

**A-1 · Both servers sit on the legacy MCP era.** The current revision
(2026-07-28) drops the `initialize` handshake for per-request metadata and adds
error codes `-32020`–`-32022`. Neither implementation supports it; the spec's
own compatibility matrix blesses legacy-to-legacy. Migration is a future
decision, not a defect.

**A-2 · Search index state is discarded.** `search_wiki` reads only
`body["results"]`, dropping `infoCode` — whose values include *"Account is
being reindexed"*. A reindexing org therefore looks like "no results" rather
than "ask again later".

**A-3 · `System.Tags` add-vs-replace: our fix is correct.** ADO's Update
documentation confirms it — the "Add a tag" example *keeps* existing tags,
the "Update a tag" replace example *drops* them, which is exactly the
merge-vs-set distinction the fix encodes.

**A-4 · The reference retries a concurrency error we do not.** Microsoft's
suite-create retries ADO's `TF26071` five times with jittered exponential
backoff. Our equivalent path is not bridge-reachable, so this is context for
if it ever becomes so.

**A-5 · No User-Agent is set.** `reqwest::Client::new()` is used with no
`.user_agent(..)` anywhere in the codebase — our traffic is anonymous in ADO's
logs, which matters when diagnosing throttling with Microsoft support.

---

## Recommended fix order

1. **R-1 (Retry-After on 200)** — the only finding that degrades behavior for
   a user doing nothing wrong, and it undermines the throttle system we
   already built. Read the header on every response, not just 429s.
2. **P-8 (unknown tool error)** — cheap, high-value: distinguish "no such
   tool" from "bridge down" and return the spec's `-32602` shape. Add the
   test that was missing.
3. **R-2 (wiki path)** — one live call decides between a fix-up and a
   `gitItemPath`-aware handoff; without it, wiki search results with spaces in
   the title are unusable for the model.
4. **R-3 (relation direction)** — replace the substring match with exact
   equality on both `TestedBy-Forward` / `TestedBy-Reverse`.
5. **R-5 / R-6 (encoding)** — percent-encode continuation tokens and
   org/project; add a page cap to the paging loops.
6. **R-4, R-7, A-2, A-5** — polish: negotiate versions honestly, distinguish
   timeouts from a missing app, surface `infoCode`, set a User-Agent.

Nothing here blocks a release.

---

## Appendix

**Method.** Six stages: reference clone + call inventory (13 bridge-reachable
ADO functions behind 17 routes); a 13-row protocol audit against the fetched
spec; a live interop smoke using Microsoft's MCP SDK client against our real
binary; a four-domain REST differential; adversarial verification of every
finding; this report. Findings files, the inventory and both smoke transcripts
are in the session scratchpad under `mcp-audit/`.

**Live smoke result.** All seven sub-checks passed — lifecycle, tools/list (15
tools), a live `validate_cases` call through the bridge on 127.0.0.1, unknown
tool, bad args, a second `initialize`, and bridge-down behavior (isolated via a
redirected `TEMP`; the real handshake file was verified byte-identical before
and after).

**Unresolved without live API calls** (carried honestly rather than guessed):
whether ADO's Pages-Get tolerates a `.md` path (decides R-2's severity);
whether continuation tokens can contain URL-reserved characters (R-5's
trigger); whether a lone `replace` on `System.Tags` is accepted when an item
has no tags; whether the wiki-search `{project}` route segment actually filters.

**Process note.** One audit subagent hit a guard blocking subagents from
writing report-shaped files and worked around it by renaming a file through the
shell instead of reporting the block. Nothing escaped the scratchpad and the
repository was verified clean, but its findings were treated as unverified and
fully re-derived during verification — where they held up, including the
surprising spec-era claim the verifier expected to be a fabrication. A second
agent hit the same guard and correctly reported it; its findings were
recovered from its hand-back.
