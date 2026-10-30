//! The document-order walk over the `Contents` record tree that records, for every area, section
//! and report object, the address of the record holding each editable field — and, for
//! structural edits, each object's contiguous byte run and each section's insertion anchor.
//!
//! The stream is a grammar in document order, not a bag of records: an *opener* record starts a
//! report object, and the attribute records around it (`0x9e` name+size, `0xbe` position, `0xc2`
//! text, `0x8` font, `0x100` font color, `0xfc` format, `0xec` border, …) decorate the most
//! recently opened one. This walk mirrors the upstream model builder's walk: same traversal
//! (pre-order = document order), same opener set, same first-wins captures.
//!
//! An address is `(tag, nth)` — exactly what `Rpt::patch_record_field` edits by. A *run* is the
//! object's whole contiguous byte span (opener or its `0xaa` wrapper through the end marker,
//! `bracket + 1`), the unit a structural edit clones or deletes; relocating a run between
//! contexts only needs a uniform re-mask (see `framing`).

use std::collections::HashMap;

use rpt_reader::raw::{Dialect, RecordNode, RecordStream};
use rpt_reader::{fields, Rpt, StreamId};
use serde::{Deserialize, Serialize};

// Record type constants (see vendor/rpt-reader/src/records/tag.rs and the field tables).
const T_AREA: u16 = 0x008a;
const T_AREA_END: u16 = 0x008b;
/// The seven band-open markers (one per area kind); BandEnd = open+1.
const T_BANDS: [u16; 7] = [0x008d, 0x008f, 0x0091, 0x0093, 0x0095, 0x0097, 0x0099];
const T_SECTION_CODE_AREA_TYPE: u16 = 0x009b;
const T_SECTION_CODE_HEADER_FOOTER: u16 = 0x009c;
const T_SECTION: u16 = 0x008c;
const T_OPEN_TEXT: u16 = 0x00a5;
const T_OPEN_FIELD: u16 = 0x009f;
const T_OPEN_SHAPE: u16 = 0x00a9;
const T_OPEN_PICTURE: u16 = 0x00ae;
const T_OPEN_SUBREPORT: u16 = 0x00a3;
const T_OPEN_CROSSTAB: u16 = 0x00b8;
const T_LINE_WRAPPER: u16 = 0x00aa;
/// `0xac` — the wrapper a BOX drawing (`0xa9`) sits inside (lines use `0xaa`, boxes `0xac`).
const T_BOX_WRAPPER: u16 = 0x00ac;
/// `0xaf` — the wrapper a STATIC picture (`0xae`) sits inside; its `0xb0` end closes the run.
const T_PICTURE_WRAPPER: u16 = 0x00af;
/// `0xb1` — the wrapper a BLOB-field picture's `0xae` sits inside.
const T_BLOB_WRAPPER: u16 = 0x00b1;
const T_NAME: u16 = 0x009e;
const T_POSITION: u16 = 0x00be;
const T_TEXT: u16 = 0x00c2;
const T_EMBEDDED_FIELD: u16 = 0x00c4;
const T_FONT: u16 = 0x0008;
const T_FONT_COLOR: u16 = 0x0100;
const T_PAGE_SETUP: u16 = 0x0066;
const T_PAGE_DEVMODE: u16 = 0x0007;
const T_OLE_OBJECT_ITEM: u16 = 0x00bd;
const T_OBJECT_FORMAT: u16 = 0x00fc;
const T_TEXT_FORMAT: u16 = 0x00c0;
const T_BORDER: u16 = 0x00ec;
const T_SECTION_COND_FORMAT: u16 = 0x00ff;
const T_SECTION_CODE: u16 = 0x009d;
const T_AREA_SECTION_FORMAT: u16 = 0x00fe;
const T_COMMON_FIELD_FORMAT: u16 = 0x00f0;
const T_STRING_FIELD_FORMAT: u16 = 0x00fa;
const T_BOOLEAN_FIELD_FORMAT: u16 = 0x00ee;
const T_NUMERIC_FIELD_FORMAT: u16 = 0x00f8;

/// Does this record type open a report object? Mirrors the model walk's opener set.
fn opens_object(rtype: u16) -> bool {
    matches!(
        rtype,
        T_OPEN_TEXT | T_OPEN_FIELD | T_OPEN_SHAPE | T_OPEN_PICTURE | T_OPEN_SUBREPORT | T_OPEN_CROSSTAB
    )
}

/// A record's address: its type tag and its 0-based occurrence among records of that type in
/// document order — the coordinates `Rpt::patch_record_field` edits by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecAddr {
    /// The record type tag (e.g. `0x00c2` for a text record).
    pub tag: u16,
    /// The 0-based occurrence of this record type in pre-order.
    pub nth: usize,
}

impl RecAddr {
    /// Address as `tag:nth`.
    pub fn key(&self) -> String {
        format!("{:#06x}:{}", self.tag, self.nth)
    }
}

/// An object's contiguous record run in the logical stream, plus the outer (ancestor-stack) mask
/// at its location — everything a clone/delete needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunInfo {
    /// Byte range [start, end): opener (or wrapper) through end marker.
    #[serde(with = "range_serde")]
    pub range: std::ops::Range<usize>,
    /// The XOR of the enclosing record types at the run's position (headers' mask there).
    pub outer_mask: u8,
}

mod range_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(r: &std::ops::Range<usize>, s: S) -> Result<S::Ok, S::Error> {
        [r.start, r.end].serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<std::ops::Range<usize>, D::Error> {
        let [start, end] = <[usize; 2]>::deserialize(d)?;
        Ok(start..end)
    }
}

/// Where a new object run is inserted into a section, and the mask there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsertAnchor {
    /// Byte offset in the logical stream to splice at.
    pub at: usize,
    /// The ancestor-stack mask at that offset.
    pub outer_mask: u8,
}

/// One located report object: the records that hold its editable fields.
#[derive(Debug, Clone, Default)]
pub struct LocatedObject {
    /// The object's name, read from its `0x9e` record (may be empty).
    pub name: String,
    /// The opener record itself (identifies the object's kind).
    pub opener: Option<RecAddr>,
    /// The opener's type — the object-kind discriminator (drawing openers are `0xa9`).
    pub opener_tag: u16,
    /// The record the run brackets at: the opener itself, or its wrapper (`0xaa` line, `0xac`
    /// box, `0xaf` static picture, `0xb1` blob-field picture). Distinguishes a static picture
    /// donor from a blob-field one — both open at `0xae`.
    pub bracket_tag: u16,
    /// `0x9e` — name + width/height.
    pub name_rec: Option<RecAddr>,
    /// `0xbe` — left/top.
    pub position: Option<RecAddr>,
    /// `0xc2` — the first literal text run.
    pub text: Option<RecAddr>,
    /// `0x08` — the first font.
    pub font: Option<RecAddr>,
    /// `0x0100` — the first font color.
    pub color: Option<RecAddr>,
    /// `0xfc` — object format (alignment, can-grow, suppress/visible).
    pub format: Option<RecAddr>,
    /// `0xec` — border (edge styles, colors, shape geometry).
    pub border: Option<RecAddr>,
    /// `0xc0` — text-object paragraph format (authoritative text alignment).
    pub text_format: Option<RecAddr>,
    /// `0xf0` — common field format (`use_system_defaults` gate).
    pub common_format: Option<RecAddr>,
    /// `0xf8` — numeric field formats (currency slot, then number slot).
    pub numeric_formats: Vec<RecAddr>,
    /// `0xfa` — string field format (word wrap, max lines, interpretation, indents).
    pub string_format: Option<RecAddr>,
    /// `0xee` — boolean field format (output type).
    pub boolean_format: Option<RecAddr>,
    /// Literal text runs (`0xc2`) in this object. 1 = simple single-run text.
    pub text_runs: usize,
    /// Embedded field references (`0xc4`) inside the text.
    pub embedded_fields: usize,
    /// The object's full contiguous byte run (opener → end marker).
    pub run: Option<RunInfo>,
    /// `0xbd` — the picture's Embedding ordinal record, when its run carries one.
    pub ordinal: Option<RecAddr>,
}

/// A located section: its own records and its objects in document order.
#[derive(Debug, Clone, Default)]
pub struct LocatedSection {
    /// The section's name.
    pub name: String,
    /// The area's name this section sits in.
    pub area_name: String,
    /// `0x8c` — the section record (height, name, object_count).
    pub section_rec: Option<RecAddr>,
    /// The section's whole band bracket (BandOpen..BandEnd) — the section clone/delete unit.
    pub band_run: Option<RunInfo>,
    /// `0xfe` — the section's own format record (suppress, page breaks, background).
    pub section_format: Option<RecAddr>,
    /// Where a new object run splices in, and the mask there.
    pub insert: Option<InsertAnchor>,
    /// The section's objects in document order.
    pub objects: Vec<LocatedObject>,
}

/// A located area.
#[derive(Debug, Clone, Default)]
pub struct LocatedArea {
    /// The area's name.
    pub name: String,
    /// The `0x8a` area record (carries the name).
    pub name_rec: Option<RecAddr>,
    /// Every `0x9b` under the `0x9c`/`0x9d` code records inside this area's bracket — each
    /// carries the area kind and group level; edits patch them all.
    pub level_recs: Vec<RecAddr>,
    /// The area's full byte run (`0x8a` through its `0x8b` AreaEnd child) — the clone unit for
    /// structural area edits.
    pub run: Option<RunInfo>,
    /// The area's sections in document order.
    pub sections: Vec<LocatedSection>,
}

/// The result of the walk.
#[derive(Debug, Clone, Default)]
pub struct Located {
    /// Areas in document order (the model re-sorts canonically; pair by name, not index).
    pub areas: Vec<LocatedArea>,
    /// `0x66` PageSetup — margins.
    pub page_setup: Option<RecAddr>,
    /// `0x07` PageDevmode — orientation, paper size.
    pub page_devmode: Option<RecAddr>,
}

/// Walk `rpt`'s `Contents` record tree in document order.
pub fn locate(rpt: &Rpt) -> Located {
    let Some(stream) = rpt.stream(&StreamId::Contents) else {
        return Located::default();
    };
    let tree = stream.record_tree();
    let mut w = Walker {
        stream,
        counters: HashMap::new(),
        located: Located::default(),
        area: None,
        section: None,
        object: None,
        ancestors: Vec::new(),
        pending_run: None,
        pending_area_run: None,
        pending_band: None,
        last_content_end: 0,
    };
    for node in &tree {
        w.visit(node);
    }
    w.close_run();
    w.located
}

struct Walker<'a> {
    stream: &'a RecordStream,
    counters: HashMap<u16, usize>,
    located: Located,
    area: Option<usize>,
    section: Option<(usize, usize)>,
    object: Option<(usize, usize, usize)>,
    /// The record-type stack of the current path (for outer masks).
    ancestors: Vec<u16>,
    /// An open object run: (object coords, bracket end-marker tag, run start, outer mask).
    pending_run: Option<((usize, usize, usize), u16, usize, u8)>,
    /// An open area bracket: (start offset, outer mask), closed by the next `0x8b`.
    pending_area_run: Option<(usize, u8)>,
    /// An open section band: (section coords, band tag, start offset, band outer mask),
    /// closed by the band's end marker (`tag+1`).
    pending_band: Option<((usize, usize), u16, usize, u8)>,
    /// content_end of the most recently fed node (run close fallback).
    last_content_end: usize,
}

impl Walker<'_> {
    fn addr(&mut self, rtype: u16) -> RecAddr {
        let nth = self.counters.entry(rtype).or_insert(0);
        let a = RecAddr { tag: rtype, nth: *nth };
        *nth += 1;
        a
    }

    fn text_field(&self, node: &RecordNode, field: &str) -> Option<String> {
        let logical = self.stream.logical_bytes();
        let read = fields::read(node, logical, Dialect::Contents)?;
        let f = read.fields.iter().find(|f| f.name == field)?;
        match &f.value {
            rpt_reader::fields::FieldValue::Text(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// The outer mask at the current position: XOR of the ancestor stack.
    fn outer(&self) -> u8 {
        self.ancestors.iter().fold(0u8, |m, t| m ^ (t & 0xff) as u8)
    }

    fn obj_mut(&mut self) -> Option<&mut LocatedObject> {
        let (a, s, o) = self.object?;
        Some(&mut self.located.areas[a].sections[s].objects[o])
    }

    /// Close the pending object run at `end` (or the last seen content end).
    fn close_run(&mut self) {
        if let Some((coords, _, _, _)) = self.pending_run {
            let end = self.last_content_end;
            if let Some(obj) = self
                .located
                .areas
                .get_mut(coords.0)
                .and_then(|a| a.sections.get_mut(coords.1))
                .and_then(|s| s.objects.get_mut(coords.2))
            {
                if let Some(run) = &obj.run {
                    obj.run = Some(RunInfo { range: run.range.start..end, outer_mask: run.outer_mask });
                }
            }
            self.pending_run = None;
        }
    }

    fn visit(&mut self, node: &RecordNode) {
        self.visit_with(node, None);
    }

    fn visit_with(&mut self, node: &RecordNode, parent: Option<&RecordNode>) {
        let addr = self.addr(node.rtype);
        let outer = self.outer();
        match node.rtype {
            T_AREA => {
                self.close_run();
                let name = self.text_field(node, "name").unwrap_or_default();
                let area_addr = addr;
                // An area's bracket runs from its `0x8a` to the NEXT `0x8b` AreaEnd — a
                // top-level sibling, not a child, so the span is closed when that arrives
                // (`pending_area_run`).
                self.pending_area_run = Some((node.offset, outer));
                self.located.areas.push(LocatedArea {
                    name,
                    name_rec: Some(area_addr),
                    level_recs: Vec::new(),
                    run: None,
                    sections: Vec::new(),
                });
                self.area = Some(self.located.areas.len() - 1);
                self.section = None;
                self.object = None;
            }
            T_AREA_END => {
                if let Some((start, outer)) = self.pending_area_run.take() {
                    if let Some(idx) = self.area {
                        self.located.areas[idx].run =
                            Some(RunInfo { range: start..node.content_end, outer_mask: outer });
                    }
                }
            }
            T_SECTION => {
                self.close_run();
                // The 0x8c's parent is its Band marker (all seven band tables declare the
                // section as their child) — open the band-bracket run for THIS section here
                // (before self.section is updated below), close at the band's tag+1.
                let band_open = parent.and_then(|p| {
                    T_BANDS.contains(&p.rtype).then(|| (p.rtype, p.offset, outer ^ (p.rtype & 0xff) as u8))
                });
                let name = self.text_field(node, "name").unwrap_or_default();
                let area_name = self
                    .area
                    .and_then(|a| self.located.areas.get(a))
                    .map(|a| a.name.clone())
                    .unwrap_or_default();
                let section = LocatedSection {
                    name,
                    area_name,
                    section_rec: Some(addr),
                    // Insert after the section record until a later anchor (the 0xff wrapper)
                    // refines it; the mask is the section record's own outer mask.
                    insert: Some(InsertAnchor { at: node.content_end, outer_mask: outer }),
                    ..Default::default()
                };
                if let Some(a) = self.area {
                    self.located.areas[a].sections.push(section);
                    let s = self.located.areas[a].sections.len() - 1;
                    self.section = Some((a, s));
                    if let Some((tag, start, band_outer)) = band_open {
                        self.pending_band = Some(((a, s), tag, start, band_outer));
                    }
                }
                self.object = None;
            }
            r if opens_object(r) => {
                self.close_run();
                // Compute the bracket BEFORE constructing the object (see the run-arm below).
                let bracket = match parent {
                    Some(p)
                        if matches!(p.rtype, T_LINE_WRAPPER | T_BOX_WRAPPER) && node.rtype == T_OPEN_SHAPE =>
                    {
                        p.rtype
                    }
                    Some(p) if (p.rtype == T_PICTURE_WRAPPER || p.rtype == T_BLOB_WRAPPER) && node.rtype == T_OPEN_PICTURE => {
                        p.rtype
                    }
                    _ => node.rtype,
                };
                let obj = LocatedObject {
                    opener: Some(addr),
                    opener_tag: node.rtype,
                    bracket_tag: bracket,
                    ..Default::default()
                };
                if let Some((a, s)) = self.section {
                    self.located.areas[a].sections[s].objects.push(obj);
                    let coords = (a, s, self.located.areas[a].sections[s].objects.len() - 1);
                    self.object = Some(coords);
                    // Drawings bracket at their `0xaa` wrapper (the wrapper's own header starts
                    // the run); everything else brackets at itself. The run's outer mask is the
                    // mask over the record its first header belongs to — for a drawing that is
                    // the WRAPPER's outer, i.e. the opener's outer less the wrapper's own type.
                    // Drawings bracket at their `0xaa`/`0xac` wrapper; pictures at their
                    // `0xaf` (static) / `0xb1` (blob-field) wrapper — the wrapper's `+1` end
                    // marker closes the run, which starts at the wrapper's own header.
                    let (bracket, run_start, run_outer) = match parent {
                        Some(p)
                            if matches!(p.rtype, T_LINE_WRAPPER | T_BOX_WRAPPER)
                                && node.rtype == T_OPEN_SHAPE =>
                        {
                            (p.rtype, p.offset, outer ^ (p.rtype & 0xff) as u8)
                        }
                        Some(p)
                            if p.rtype == T_PICTURE_WRAPPER || p.rtype == T_BLOB_WRAPPER
                                && node.rtype == T_OPEN_PICTURE =>
                        {
                            (p.rtype, p.offset, outer ^ (p.rtype & 0xff) as u8)
                        }
                        _ => (node.rtype, node.offset, outer),
                    };
                    let run = RunInfo { range: run_start..run_start, outer_mask: run_outer };
                    let sec = &mut self.located.areas[a].sections[s];
                    if let Some(obj) = sec.objects.last_mut() {
                        obj.run = Some(run);
                    }
                    self.pending_run = Some((coords, bracket.wrapping_add(1), run_start, run_outer));
                }
            }
            T_NAME => {
                let name = self.text_field(node, "name").unwrap_or_default();
                if let Some(obj) = self.obj_mut() {
                    obj.name_rec = Some(addr);
                    obj.name = name;
                }
            }
            T_POSITION => {
                if let Some(obj) = self.obj_mut() {
                    if obj.position.is_none() {
                        obj.position = Some(addr);
                    }
                }
            }
            T_TEXT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.text.is_none() {
                        obj.text = Some(addr);
                    }
                    obj.text_runs += 1;
                }
            }
            T_EMBEDDED_FIELD => {
                if let Some(obj) = self.obj_mut() {
                    obj.embedded_fields += 1;
                }
            }
            T_FONT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.font.is_none() {
                        obj.font = Some(addr);
                    }
                }
            }
            T_FONT_COLOR => {
                if let Some(obj) = self.obj_mut() {
                    if obj.color.is_none() {
                        obj.color = Some(addr);
                    }
                }
            }
            T_OBJECT_FORMAT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.format.is_none() {
                        obj.format = Some(addr);
                    }
                }
            }
            T_TEXT_FORMAT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.text_format.is_none() {
                        obj.text_format = Some(addr);
                    }
                }
            }
            T_BORDER => {
                if let Some(obj) = self.obj_mut() {
                    if obj.border.is_none() {
                        obj.border = Some(addr);
                    }
                }
            }
            T_STRING_FIELD_FORMAT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.string_format.is_none() {
                        obj.string_format = Some(addr);
                    }
                }
            }
            T_BOOLEAN_FIELD_FORMAT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.boolean_format.is_none() {
                        obj.boolean_format = Some(addr);
                    }
                }
            }
            T_COMMON_FIELD_FORMAT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.common_format.is_none() {
                        obj.common_format = Some(addr);
                    }
                }
            }
            T_NUMERIC_FIELD_FORMAT => {
                if let Some(obj) = self.obj_mut() {
                    if obj.numeric_formats.len() < 2 {
                        obj.numeric_formats.push(addr);
                    }
                }
            }
            T_SECTION_CODE | T_SECTION_COND_FORMAT => {
                // The section's code and condition-format records are the top-level siblings
                // between the band bracket and the objects; inserting after the last of them
                // matches the engine's sibling order (section, section code, formats,
                // objects…). Both are top-level records, so the anchor is never nested inside
                // the band — a section with no objects and no formats still lands correctly
                // after its SectionCode.
                if self.object.is_none() {
                    if let Some((a, s)) = self.section {
                        self.located.areas[a].sections[s].insert =
                            Some(InsertAnchor { at: node.content_end, outer_mask: outer });
                    }
                }
            }
            T_AREA_SECTION_FORMAT => {
                // While a section is current and no object has opened, the first `0xfe` is the
                // section's own format (the area's came before any section).
                if self.object.is_none() {
                    if let Some((a, s)) = self.section {
                        let sec = &mut self.located.areas[a].sections[s];
                        if sec.section_format.is_none() {
                            sec.section_format = Some(addr);
                        }
                    }
                }
            }
            T_SECTION_CODE_AREA_TYPE => {
                // The `0x9b`s nested in the `0x9c`/`0x9d` code records carry the area kind
                // and, for group areas, the nesting level.
                if parent.is_some_and(|p| {
                    p.rtype == T_SECTION_CODE_HEADER_FOOTER || p.rtype == T_SECTION_CODE
                }) {
                    if let Some(a) = self.area {
                        self.located.areas[a].level_recs.push(addr);
                    }
                }
            }
            t if T_BANDS.contains(&t) || T_BANDS.iter().any(|b| t == b.wrapping_add(1)) => {
                // Band end marker: close the section's band-bracket run. (Band OPEN markers
                // are handled as the 0x8c's parent; they carry nothing themselves.)
                if T_BANDS.iter().any(|b| t == b.wrapping_add(1)) {
                    if let Some(((a, s), _, start, band_outer)) = self.pending_band.take() {
                        if let Some(sec) = self
                            .located
                            .areas
                            .get_mut(a)
                            .and_then(|ar| ar.sections.get_mut(s))
                        {
                            sec.band_run =
                                Some(RunInfo { range: start..node.content_end, outer_mask: band_outer });
                        }
                    }
                }
            }
            T_PAGE_SETUP => {
                if self.located.page_setup.is_none() {
                    self.located.page_setup = Some(addr);
                }
            }
            T_OLE_OBJECT_ITEM => {
                if let Some(obj) = self.obj_mut() {
                    if obj.ordinal.is_none() {
                        obj.ordinal = Some(addr);
                    }
                }
            }
            T_PAGE_DEVMODE => {
                if self.located.page_devmode.is_none() {
                    self.located.page_devmode = Some(addr);
                }
            }
            _ => {}
        }

        // Close the pending run when its end marker arrives.
        if let Some((_, end_tag, _, _)) = self.pending_run {
            if node.rtype == end_tag {
                let end = node.content_end;
                self.close_run_at(end);
            }
        }

        self.last_content_end = node.content_end;
        self.ancestors.push(node.rtype);
        for child in &node.children {
            self.visit_with(child, Some(node));
        }
        self.ancestors.pop();
    }

    fn close_run_at(&mut self, end: usize) {
        if let Some((coords, _, start, outer)) = self.pending_run {
            if let Some(obj) = self
                .located
                .areas
                .get_mut(coords.0)
                .and_then(|a| a.sections.get_mut(coords.1))
                .and_then(|s| s.objects.get_mut(coords.2))
            {
                obj.run = Some(RunInfo { range: start..end, outer_mask: outer });
            }
            self.pending_run = None;
        }
    }
}
