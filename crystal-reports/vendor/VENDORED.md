# Vendored from rpt-rs

- Upstream: https://github.com/MrSrsen/rpt-rs
- Commit pinned: `3f5beb51dd34d36c4a4326280d25f473b44889d1` (main, cloned 2026-10-15)
- License: MPL-2.0 (see `LICENSE`). Third-party notices: `THIRD-PARTY-NOTICES.md`.

## What is vendored

The reader/writer crates, and — as of the preview-rendering batch — the render pipeline:

- `rpt-model` — the format-neutral semantic model types (with the `serde` feature enabled).
- `rpt-test-support` — the upstream dev-only test helpers (fixtures paths, SavedData builders,
  finished-PDF inspection); carries no local changes.
- `rpt-reader` — container/codec/record reader, field tables, and the byte-level write path
  (`Rpt::reencode`, `Rpt::patch_record_field`, `EditPolicy`).
- The render stack (preview rendering, saved-data-only; see #10 below): `metafile` (the one
  MIT/Apache-2.0 crate — its LICENSE files are vendored alongside), `rpt-format-value`,
  `rpt-formula`, `rpt-data`, `rpt-pages`, `rpt-text` (+ its `fonts/` directory: Liberation
  faces under SIL OFL 1.1, DejaVu Sans under the Bitstream Vera licence — both licence texts
  are in the fonts directory and MUST travel with it), `rpt-render-util`, `rpt-render-pdf`,
  `rpt-layout`, `rpt-render`.
- `tests/fixtures/` at the workspace root — the report corpus and render baselines the
  vendored crates' tests resolve through `rpt_test_support::workspace_root()` (the upstream
  repo's own layout). `sql/` is not vendored: its only consumer was the pruned
  `postgres_fixtures.rs`.

Everything else in the upstream workspace (DB drivers, KDL/JSON export, the two CLI apps) is
not vendored. Changes made to the vendored copies, so they build outside the upstream workspace:

- Crate `Cargo.toml`s keep their upstream `{ workspace = true }` dependency form; this
  workspace's root manifest carries the corresponding path entries and the same third-party
  pins the upstream root uses (fontdb 0.23, ttf-parser 0.25, cosmic-text 0.19, krilla 0.8.2,
  harfrust 0.5, similar 3).
- The `[lints] workspace = true` inheritance resolves against this workspace's root manifest,
  which carries the upstream lint table verbatim.

Source changes — ten documented extensions (each marked "Local extension" at its site; #10 is
the render-stack vendoring itself):

1. `fields.rs`: `FieldEdit::Ref { text, kind, index }` — a field-reference edit value, encoded
   by the table's own `write_field_ref` and verified by the same round-trip gate as every other
   named field. Enables verified rebinds for addField/addSpecialField.
2. `io/edit.rs`: `Rpt::write_contents_logical(&[u8])` — write replacement logical `Contents`
   bytes (the seam structural editors splice through).
3. `records/mod.rs`: `RecordStream::decode` is now `pub` — decoding a nested
   `Subdocument N/Contents` stream's raw bytes for walking (the reader's own subreport path).
4. `codec/qe_crypto.rs`: `qe_cfb_encrypt` (new, the inverse of the existing decrypt on the
   same block cipher and fixed key schedule) and `qe_cfb_decrypt` made `pub` — the QESession
   re-encoder for the table operations.
5. `codec/mod.rs`: the `qe_crypto` module is `pub` so the two functions can be re-exported at
   the crate root.
6. `build_model/report_def/formats.rs`: `apply_object_format` now populates the model's
   `tooltip_text`/`css_class` (fields the upstream model declares but never filled), so the
   decoded model is a faithful mirror of the `0x00fc` record.
7. `build_model/report_def/objects.rs`: `reclassify_picture_openers` no longer reclassifies a
   picture as blob-field when it carries a `0xbd` Embedding ordinal — the old heuristic keyed
   on the object's NAME starting with "Picture", misclassifying legitimately-named static
   pictures (`RO_Xtreme_Logo` in the vendored World Sales fixture).
8. `model/fit.rs`: `SpecialFieldType::from_code` carries the engine-verified wire table's
   0x08–0x0F block (RecordSelection/GroupSelection/FilePath/FileAuthor/FileCreationDate/
   RecordNumber/GroupNumber/TotalPageCount per the engine's special-field table).
9. `io/edit.rs`: `Rpt::write_stream_logical(&StreamId, &[u8])` — re-encode ANY Contents-dialect
   stream's logical bytes (the nested-`Subdocument N/Contents` twin of
   `write_contents_logical`).
10. The render stack, vendored whole for preview rendering (`rpt-core::preview`). All ten
    crates are the pinned commit's sources unchanged EXCEPT: `rpt-render`'s dev-dependency on
    `rpt-db-postgres` and its `tests/postgres_fixtures.rs` are pruned (the vendor set renders
    from saved data only and carries no DB crate) — every other upstream test is kept and runs
    in the workspace gate. No render-source modifications: the vendored `rpt-reader`/`rpt-model`
    (with extensions #1–#9) slot in through the workspace dependency table, and the fidelity
    question was settled empirically — rendering the World Sales fixture with the
    upstream-built `rpt-render` binary vs this stack differs only in the Producer version
    string and one picture OUR side renders correctly (extension #7's fix); the golden-PDF
    hash test in `rpt-core/tests/preview.rs` pins the output.

11. `rpt-layout` (`lib.rs`, `paginate.rs`): a report whose group bands produced no group
    instance (an empty dataset) prints NO group header or footer — engine parity with
    Crystal, which suppresses every group band when there are no rows. Upstream instead laid
    out the group skeleton once ("so static content renders"), which evaluated group-band
    field objects over zero rows and surfaced them as unknown-name formula errors. The
    upstream test pinning the skeleton behavior was rewritten to pin the parity behavior
    (`empty_dataset_prints_no_group_bands`).

12. `rpt-layout` (new `src/html.rs` + `src/paginate.rs`): Crystal's **HTML text
    interpretation** (`StringFieldFormat.TextFormat == HTMLText`, stored `0x00fa` byte 15).
    A field marked HTML renders its resolved value as markup instead of literal text —
    document wrappers (`<html>/<head>/<meta>/<body>`) invisible, `<br>`/`<p>` line breaks,
    entities decoded, `<b>/<i>/<u>/<s>`, `<span style='font-size:…'>` and `<font
    size=/face=>` styles applied at the line's first character (the line model carries one
    font per line, matching the corpus's authored `<br>`-before-style-change pattern).
    Unknown tags are dropped, their text kept. Upstream ignored the stored flag entirely and
    printed the markup literally.

The MPL-2.0 requires these files to remain under MPL-2.0 with their notices when they are
further distributed — keep this directory intact when copying into your application, including
this list of local changes.
