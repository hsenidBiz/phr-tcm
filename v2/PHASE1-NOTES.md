# Phase 1 exit notes — read-only browse (2026-07-11)

Branch: `feat/tauri-rewrite`. Verdict: **GO** for Phase 2 planning.

## Delivered

- **Silent token refresh**: `TokenSet` (access + refresh + expiry + account)
  in Rust memory only — deliberately no serde/specta derives, so it cannot
  cross IPC. `get_fresh_token` renews via the refresh_token grant 5 min
  before expiry; refresh failure falls back to the current token so a hard
  401 surfaces as `Unauthorized` (UI says sign in again).
- **ADO reads** ported from v1 `devops_client.py` with wiremock coverage:
  - `list_orgs` — two-hop vssps discovery (profiles/me → accounts), sorted
    case-insensitively.
  - `search_pbis` — WIQL POST (query-only), title CONTAINS with `'`→`''`
    escaping, OR exact-id when numeric, PBIs only, WIQL order preserved.
  - `get_pbi_test_cases` — `$expand=relations`, keeps rels containing
    `testedby`, batch-GETs fields in chunks of 200.
- **Browse UI**: org select → project select → PBI search (Enter) → linked
  test case table (id/title/tags/automation), all TanStack Query.
- Tests: 19 Rust + 5 Vitest, all green; frontend build clean.

## Gotchas added this phase

- RTL auto-cleanup does not register under vitest without `globals: true` —
  `test-setup.ts` now calls `cleanup()` in `afterEach`, otherwise each test's
  DOM leaks into the next (met as "multiple elements found").

## Still pending / carried risks

- Live AAD flow still pending user verification (`cd v2; npm run tauri dev`),
  now also covering: org list loads for the real account, PBI search returns
  real hits, test case table fills.
- No rate limiter yet (needed when Phase 2 introduces writes: 2/s budget).
- CAE / conditional-access behavior unknown until the live run.
- WebView2 bootstrap + frameless chrome unchanged (later phases).

## Phase 2 preview (next plan)

Test Case CRUD + xlsx import: port `import_parser.py` + `xml_builder.py`
against the existing pytest suites as golden vectors (calamine +
rust_xlsxwriter + quick-xml), first POST/PATCH writes with the token-bucket
limiter, queue UI.
