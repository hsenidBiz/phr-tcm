# The `crpt-ir/1` JSON IR

Produced by `rpt-core::ir::build_ir` / `rpt-tools ir`, consumed by the editor UI. Deterministic
for the same input. TS types: `ui-react/src/types.ts` (kept in sync).

```jsonc
{
  "format": "crpt-ir/1",
  "file": "C:/reports/Sample.rpt",
  "summary": { "title": null, "subject": null, "author": null, "comments": null, "last_author": null },
  "version": 1,                       // stream-envelope encryption version word
  "authoring_version": "V11",         // the CR version that wrote the file, as stored
  "print_options": {                  // model PrintOptions verbatim (margins in twips, enums as strings)
    "margins": { "left": 360, "right": 360, "top": 360, "bottom": 360 },
    "paper_orientation": "Portrait", "paper_size": "DefaultPaperSize", "..."
  },
  "areas": [                          // canonically ordered: RH → PH → GH* → Detail → GF* → RF → PF
    {
      "name": "PageFooterArea1", "kind": "PageFooter", "group_level": null,
      "sections": [
        {
          "name": "PageFooterSection1", "height": 420, "width": 0, "suppress": false,
          "object_ids": ["o79", "o80", "o81", "o82"],
          "address": { "tag": 140, "nth": 6 }   // the 0x8c section record
        }
      ]
    }
  ],
  "objects": [                        // flat, id-addressed; ids stable across edit batches
    {
      "id": "o81", "name": "FtConf", "kind": "Text",
      "area": "PageFooterArea1", "section": "PageFooterSection1",
      "bounds": { "left": 0, "top": 140, "width": 6000, "height": 220 },   // twips
      "text": {                        // present for Text / FieldHeading / Field objects
        "text": "CONFIDENTIAL - …", "display": "CONFIDENTIAL - …", "editable": true,
        "font": { "name": "Arial", "size_pt": 8.0, "bold": false, "italic": false,
                  "underline": false, "strikethrough": false, "weight": 400 },
        "color": { "r": 107, "g": 114, "b": 128, "a": 255 }
      },
      "field": null,                   // { data_source, ref_kind } for Field objects
      "subreport": null,               // name for Subreport objects
      "address": {                     // record addresses backing each edit (None = not editable)
        "name":    { "tag": 158, "nth": 2 },    // 0x009e: name/width/height
        "position":{ "tag": 190, "nth": 2 },    // 0x00be: left/top
        "text":    { "tag": 194, "nth": 0 },    // 0x00c2: literal text
        "font":    { "tag": 8,   "nth": 1 },    // 0x0008: font
        "color":   { "tag": 256, "nth": 1 }     // 0x0100: text color
      }
    }
  ],
  "subreports": ["companylogo"],
  "field_names": ["emp_display_number", "...", "@performance_cycle_id", "..."],
  "warnings": []                       // pairing/decode warnings; affected items are not editable
}
```

## Units

- All geometry: **twips** (1440 = 1 inch = 72 points; 20 twips = 1 point).
- Font sizes: **points** (may be fractional — the record stores twips).
- Colors: 0–255 RGBA.

## Edit ops

A batch is a JSON array; `rpt-tools edit <in> <ops.json> <out>` or the `apply_edits` Tauri
command. Ops apply in order; a refused op aborts the batch (earlier ops stand) with a message.

```jsonc
[
  // text — single-run objects only (IR text.editable == true)
  { "op": "set_text",        "object": { "id": "o81" }, "text": "New footer" },
  // object selector: one of id | name (must be unique) | index

  { "op": "set_object_name", "object": { "id": "o81" }, "name": "FooterConf" },
  // caveat: renames do not fix references — a field-heading's link names its object, and
  // subreport links may reference names. Renaming a referenced object desyncs that reference.

  // position / size (twips; omit what stays)
  { "op": "set_bounds",      "object": { "id": "o81" }, "left": 200, "top": 160, "width": 7000, "height": 300 },

  // font (omit what stays); bold writes weight 700/400 unless weight given
  { "op": "set_font",        "object": { "id": "o81" }, "name": "Tahoma", "size_pt": 10.5, "bold": true },

  // text color
  { "op": "set_text_color",  "object": { "id": "o81" }, "r": 200, "g": 0, "b": 0 },

  // section height (twips)
  { "op": "set_section_height", "section": { "area": "PageFooterArea1", "section": "PageFooterSection1" }, "height": 500 },

  // page margins (twips; omit what stays)
  { "op": "set_margins",     "left": 720, "right": 720, "top": 720, "bottom": 720 },

  // orientation / paper — only when the report carries a page-DEVMODE record
  // (saved with a printer); otherwise refused with an explanation
  { "op": "set_orientation", "orientation": "landscape" },
  { "op": "set_paper_size",  "paper": 9 }      // DMPAPER id: 1=letter, 9=A4, 11=A5 …
]
```

## Safety model

- Every field edit is verified by the writer: the record's field table must reproduce the record
  byte-for-byte, and the written file must read back the exact change. Margins/orientation/paper
  ride a documented page-level path (fixed-width head fields + explicit read-back verification).
- Only `Contents` is rewritten; all other streams are copied verbatim.
- No op adds or removes records ⇒ `(tag, nth)` addresses and IR object ids are stable across
  batches.
