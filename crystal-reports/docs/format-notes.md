# Crystal Reports .rpt — format notes (as verified in this workspace)

Everything here was verified against the 6 PMSV10 samples and 263 designer-saved sample reports
(CR XI R2, 11.5) in this workspace, using the vendored `rpt-reader` and its field tables. The
authoritative long-form spec is upstream: `rpt-rs/docs/format/` (see `vendor/VENDORED.md`).

## Container

A `.rpt` is an **OLE2/CFB** compound document (magic `D0 CF 11 E0 …`, 512-byte sectors). Streams:

| stream | holds |
|---|---|
| `Contents` | the report definition — TSLV record tree |
| `QESession` | query-engine session: SQL, connections, tables (`QENG` header) |
| `PromptManager` | parameter prompts (XML after decode) |
| `ReportInfo` | 58 bytes of feature flags |
| `Subdocument N` | a subreport's own Contents/QESession/… (a storage) |
| `zlibBLOB N` / `BLOB 1l` | embedded images (zlib'd, encrypted) |
| `\x05SummaryInformation` | OLE properties + preview thumbnail DIB |
| `Embedding N` | OLE object bytes |

## Stream encoding (Contents)

`0xFFFF` header record → `{isEncrypted, version, useFixedKey, IV}` → AES-128 in a **custom CFB
variant** (transposed state, two fixed published engine keys) → **zlib** → **TSLV records**.

TSLV record: flag byte (`0xF8`/`0xF9`), type (`u16`), schema word (`u16`, BE, per-type version),
length, content; content strings are XOR-masked by a per-record mask; records nest by end-marker
(child of type `T+1`). The vendored reader/writer handles all of this; nothing in an editor needs
to touch it directly.

## The record grammar around editable objects

`Contents` is a grammar in **document order**. An *opener* starts a report object; attribute
records around it decorate the most recently opened object:

```
Area (0x8a) "PageFooterArea1"
  Band (0x91/0x8d/0x8f/0x93/0x95/0x97/0x99)
    Section (0x8c) "PageFooterSection1"        ← height, name
  … then, as siblings between Band and BandEnd:
  TextObjectContainer (0xa5)                   ← opener (text object)
    ObjectName (0x9e) "FtConf"                 ← NAME + WIDTH + HEIGHT (nested child)
  ObjectPosition (0xbe)                        ← LEFT + TOP (sibling after opener)
  ObjectConditionFormat (0xfd) → ObjectFormat (0xfc)     (suppress, alignment, …)
  ObjectAdornment (0xed) → ObjectBorder (0xec)          (borders, colors)
  TextObjectFormat (0xc0)                      ← paragraph format
  TextObject (0xc2)                            ← LITERAL TEXT (one per run)
  FontConditionFormat (0x101) → FontColor (0x100)       ← text color [type,B,G,R] BE u32
  Font (0x8)                                   ← face, size, weight, italic, …
  TextRunEnd (0xc3) / ParagraphEnd (0xc1) / TextObjectEnd (0xa6)
```

Opener types: `0xa5` text, `0x9f` field, `0xa9` drawing (nested in `0xaa`), `0xae` picture (also a
chart's graphic base), `0xa3` subreport, `0xb8` cross-tab.

Facts an editor must respect (all mirrored in `rpt-core`):

- **Objects are siblings in the band bracket**, not children of the section record.
- **First font / first color win**: the engine reports an object's font/color as its *first run's*;
  later runs may override per-run.
- **`0xc2` text**: one record per literal run. `text` (model) = the *last* run's content;
  `display` = all runs joined. Single-run objects are the only safe v1 edit target.
- **Font size** is stored twice — `size_points` (u16, whole points) and, when present,
  `size_twips` (i32, 20 per point, the engine's preferred source). Patch both coherently
  (`set_font` does).
- **Weight**, not bold: 400 = normal, ≥700 reads as bold.
- **Color wire form**: BE u32 `[type, blue, green, red]`, type `0x00` = plain color.
- **Margins** (`0x66`): four i32 twips *in the order left, right, top, bottom*; `i32::MIN`
  (`0x80000000`) is the engine's "use default" sentinel (resolves to 360 twips). The record's tail
  beyond the margins is only partially modelled — edit via the page-level path with read-back
  verification (see `rpt-core/src/edit.rs`).
- **Orientation / paper size** (`0x07` PageDevmode, a compacted BE DEVMODE): present only when the
  report was saved with a printer. Orientation 1 = portrait, 2 = landscape; paper = DMPAPER id.
  The `0x18e` PaperRect holds paper width/height in twips and is *not* re-oriented by the engine.
- **Units**: everything geometric is twips (1440 = 1 inch = 72 pt); coordinates are
  section-relative; page setup margins are page-relative.

## What the editor's writer does

Only `Contents` is ever rewritten (TSLV re-frame → deflate → encrypt → CFB splice); every other
stream is copied verbatim. Field edits are verified per record: the record's field table must
reproduce it byte-for-byte and the written file must read back exactly the requested change.
No edit adds or removes records, so `(tag, nth)` addresses stay stable across batches.

## Version scope

Validated on XI R2 (11.5) files — stream header `ver=1`, record schemas `0x0700/0x0701`,
`QESession` schema `0x0902`. The upstream corpus (154 reports) extends to CR 2013-era files;
older (8.5-era, unencrypted) files use different schemas and are untested here.
