# MCP & ADO REST Audit — Design

**Goal:** Establish, with evidence, whether the app's MCP implementation (the
hand-rolled `v2.exe --mcp` stdio layer + bridge tool schemas) and its Azure
DevOps REST usage are sound — by differential audit against
`microsoft/azure-devops-mcp` (Microsoft's official MCP server) and the MCP
specification, plus one live client-interop smoke. **Deliverable is a findings
report; no production code changes in this effort.**

## Why this shape (approach C, user-approved)

The two layers have opposite risk profiles:

- Our **protocol layer is hand-rolled** (`v2/src-tauri/src/mcp.rs` speaks
  newline-delimited JSON-RPC over stdio itself; the reference gets
  spec-conformance for free from the official MCP TypeScript SDK). Only
  Claude-family clients have ever spoken to it. Static reading cannot prove
  interop — so this layer gets BOTH a spec audit and a live smoke driven by
  the official SDK's own client.
- Our **REST layer is heavily wiremock-pinned** (request shapes asserted in
  `v2/src-tauri/tests/ado*.rs` and friends). The open question is whether
  those pinned shapes match what Microsoft's own tooling sends — a static
  source-to-source diff answers that without touching the real org. The
  recently-found `System.Tags` add-merges quirk is exactly the class of bug
  this lane exists to catch.

Approach B's side-by-side live traffic diff against the real org was
considered and dropped: it spends rate-limit budget mostly re-proving what
wiremock already pins.

## Components

### 0. Reference acquisition

Shallow-clone `https://github.com/microsoft/azure-devops-mcp` into the
session scratchpad (never into this repo). Record the cloned commit hash in
the report — the audit is against a snapshot, and findings must say which.

### 1. Protocol lane — static spec audit

Audit `mcp.rs` (and the stdio proxy entry in `main.rs`) against the MCP
specification, requirement by requirement, with the reference SDK's observable
behavior as tiebreaker where the spec is loose. The checklist (each row gets a
verdict + evidence):

- **Framing:** newline-delimited JSON; no embedded newlines in messages;
  UTF-8; behavior on oversized/garbage lines.
- **Lifecycle:** `initialize` request → response carries `protocolVersion`
  (negotiation behavior when the client's version differs), `capabilities`,
  `serverInfo`; `notifications/initialized` accepted and not answered;
  behavior on stdin EOF (clean shutdown); requests before initialize.
- **tools/list:** shape of each tool entry (`name`, `description`,
  `inputSchema` as valid JSON Schema); stability of the list.
- **tools/call:** result as `content` array; tool-level failures reported via
  `isError: true` content rather than JSON-RPC errors; unknown tool name;
  arguments that violate the schema.
- **JSON-RPC discipline:** id echo (string and number ids); `-32700` parse
  error, `-32600` invalid request, `-32601` method not found, `-32602`
  invalid params; unknown methods; notifications (no id) never answered.
- **Bridge-down behavior:** the proxy's answers when the app is not running
  or the handshake file is stale — errors must be well-formed MCP errors,
  not stdio garbage.

Much of this is already pinned by `tests/tcm_mcp.rs`; the audit maps each
spec requirement → our code/test evidence → verdict, and the gaps (spec rows
with no covering test) are themselves findings.

### 2. Protocol lane — live client-interop smoke

A small Node script in the scratchpad using `@modelcontextprotocol/sdk`'s
`Client` + `StdioClientTransport`, spawning the real dev-built `v2.exe --mcp`:

1. Full lifecycle: connect → initialize → initialized → `tools/list`.
2. Happy call: a tool that answers without sign-in (`check_spec_coverage`
   with an inline draft + spec text, or `validate_cases` with an inline
   draft) — asserts a well-formed `content` result.
3. Error paths: unknown tool; schema-violating arguments; a second
   `initialize`.
4. Transcript captured verbatim into the report as evidence.

Preconditions: the app running (bridge up); sign-in NOT required (the chosen
tools answer cold). If the smoke cannot run (SDK install failure, app not
available), the report says so explicitly and the protocol verdicts are
downgraded to static-only confidence — never silently.

The script is audit tooling, not product: it lives in the scratchpad and is
reproduced in the report's appendix. Promoting it to a repeatable
`scripts/` conformance gate is a candidate follow-up, not part of this
effort.

### 3. REST lane — static differential audit

Inventory every ADO REST call reachable from the bridge tools plus the core
flows they depend on (`ado/endpoints.rs`, `ado_testplan/*`, wiki search,
`ado_share`, attachments/screenshots). For each call, locate the reference's
equivalent (its `src/` is organized by domain — test-plans, work-items,
search, wiki cover our overlap) and diff:

- **api-version** (ours are pinned 7.1; theirs may differ or use previews),
- **endpoint + HTTP method** (e.g. our `GET workitems?ids=` chunked at 200
  vs a possible `POST workitemsbatch`),
- **parameters and encoding** (project names with spaces, `$expand`, field
  lists),
- **semantics** (patch op choices — the Tags class of bug; WIQL usage;
  suite/point/result flows),
- **paging** (do they honor `x-ms-continuationtoken` / `$top` loops where we
  read once? an un-paged list that silently truncates at the server's default
  page size is a real finding),
- **error/retry** (our typed 401/429/403/404 mapping and Retry-After honor
  vs theirs).

Where the two implementations differ, the ADO REST documentation is the
norm; the reference is evidence of intent, not automatically correct.

### 4. Verification pass

Every candidate finding is adversarially verified before it enters the
report (re-read both sides; for REST semantics, check against ADO's docs).
Unverifiable claims are labeled as such or dropped. No finding ships on a
single reading.

### 5. The report

`claudedocs/mcp-audit-2026-08.md` (project convention: reports live in
`claudedocs/`). Contents:

- Summary: overall soundness verdict per lane, in plain language.
- Findings register: one row per finding — id, lane, severity (**broken** /
  **risky divergence** / **benign divergence** / **info**), our evidence
  (file:line), reference or spec evidence (file:line / spec section),
  recommended action.
- The live smoke transcript (appendix) and the reference commit hash.
- A prioritized fix list the user can pick from — fixes are a follow-up
  plan, not this effort.

## Error handling

- Reference clone fails → stop and report; no audit against memory.
- Live smoke fails to launch → static-only confidence, stated in the report.
- A spec area our implementation deliberately does not support (e.g.
  resources/prompts capabilities) is **info**, not a defect, provided the
  capability advertisement is honest — advertising what we don't serve IS a
  defect.

## Out of scope

- Tool coverage/design comparison (user declined this lane).
- Any production code change, including "quick" fixes for findings.
- Live traffic diff against the real org (approach B).
- Reference domains with no overlap with our tools (pipelines,
  advanced-security, repositories).

## Success criteria

Every bridge-reachable ADO call and every MCP protocol checklist row has a
verdict backed by evidence; the live smoke transcript shows a spec-conformant
third-party client completing lifecycle + a real tool call against our
binary; the report's fix list is concrete enough that each item could become
a plan task without re-investigation.
