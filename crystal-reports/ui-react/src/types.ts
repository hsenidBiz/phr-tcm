/**
 * TypeScript types for the `crpt-ir/1` IR and the edit-op vocabulary.
 * Kept in sync with Out/rpt-core/src/ir.rs and Out/rpt-core/src/edit.rs.
 * All lengths are twips (1440 = 1 inch); font sizes in points.
 */

export const IR_FORMAT = "crpt-ir/1";

export interface RecAddr {
  tag: number; // record type, e.g. 0x00c2 (194)
  nth: number; // 0-based occurrence of that type in document order
}

export interface SummaryIr {
  title: string | null;
  subject: string | null;
  author: string | null;
  comments: string | null;
  last_author: string | null;
}

export interface SectionIr {
  name: string;
  height: number;
  width: number;
  suppress: boolean;
  object_ids: string[];
  address: RecAddr | null;
}

export interface AreaIr {
  name: string;
  kind: string; // ReportHeader | PageHeader | GroupHeader | Detail | GroupFooter | ReportFooter | PageFooter
  group_level: number | null;
  sections: SectionIr[];
}

export interface BoundsIr {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface FontIr {
  name: string;
  size_pt: number;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  strikethrough: boolean;
  weight: number;
}

export interface ColorIr {
  r: number;
  g: number;
  b: number;
  a: number;
}

export interface TextIr {
  text: string;
  display: string;
  editable: boolean; // false = has embedded field refs; set_text refused
  font: FontIr;
  color: ColorIr;
}

export interface FieldIr {
  data_source: string; // e.g. "{Command.name}", "{@formula}", "Page N of M"
  ref_kind: string; // DatabaseField | Formula | Parameter | Special | Summary | ...
}

export interface AddressIr {
  name: RecAddr | null;
  position: RecAddr | null;
  text: RecAddr | null;
  font: RecAddr | null;
  color: RecAddr | null;
}

export interface ObjectIr {
  id: string; // "o12" — stable within a snapshot and across edit batches
  name: string;
  kind: string; // Text | FieldHeading | Field | Box | Line | Picture | Subreport | Chart | CrossTab | ...
  area: string;
  section: string;
  bounds: BoundsIr;
  text: TextIr | null;
  field: FieldIr | null;
  subreport: string | null;
  address: AddressIr;
}

export interface Margins {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export interface PrintOptions {
  margins: Margins;
  paper_orientation: string; // Portrait | Landscape
  paper_size: string;
  content_width: number;
  content_height: number;
  [k: string]: unknown;
}

export interface Ir {
  format: typeof IR_FORMAT;
  file: string;
  summary: SummaryIr;
  version: number;
  authoring_version: string;
  print_options: PrintOptions;
  areas: AreaIr[];
  objects: ObjectIr[];
  subreports: string[];
  /** The report's groups, outermost first (the `addGroup` view). */
  groups: GroupIr[];
  /** The record sort order (group sorts first, then explicit sorts). */
  sorts: SortIr[];
  /** Database fields the report's data source exposes — the `addField` allowlist
   *  (offline: fields the report already references). */
  available_fields: AvailableFieldIr[];
  field_names: string[];
  warnings: string[];
}

/** One group, as the grouping operations see it. */
export interface GroupIr {
  field_ref: string;
  direction: "ascending" | "descending" | "unknown";
  header_section: string | null;
  footer_section: string | null;
}

/** One record sort. */
export interface SortIr {
  field_ref: string;
  direction: string;
}

/** One bindable field of the data source (the `addField` vocabulary). */
export interface AvailableFieldIr {
  formula_form: string;
  name: string;
  table_alias: string;
  value_type: string;
}

/* ---------- edit ops (mirrors EditOp in edit.rs) ---------- */

export interface ObjSel {
  id?: string;
  name?: string;
  index?: number;
}

export interface SecSel {
  area: string;
  section: string;
}

export type EditOp =
  | { op: "set_text"; object: ObjSel; text: string }
  | { op: "set_object_name"; object: ObjSel; name: string }
  | {
      op: "set_bounds";
      object: ObjSel;
      left?: number;
      top?: number;
      width?: number;
      height?: number;
    }
  | {
      op: "set_font";
      object: ObjSel;
      name?: string;
      size_pt?: number;
      bold?: boolean;
      italic?: boolean;
      underline?: boolean;
      strikeout?: boolean;
      weight?: number;
    }
  | { op: "set_text_color"; object: ObjSel; r: number; g: number; b: number }
  | { op: "set_section_height"; section: SecSel; height: number }
  | {
      op: "set_margins";
      left?: number;
      right?: number;
      top?: number;
      bottom?: number;
    }
  | { op: "set_orientation"; orientation: "portrait" | "landscape" }
  | { op: "set_paper_size"; paper: number } // DMPAPER id: 1=letter, 9=A4, 11=A5...
  // --- VibeyReports layout operations (native serde shapes; colours "#RRGGBB") ---
  | { op: "set_alignment"; object: ObjSel; alignment: string }
  | { op: "set_fill_color"; object: ObjSel; color: string }
  | { op: "set_line_color"; object: ObjSel; color: string }
  | { op: "set_section_background"; section: SecSel; color: string }
  | {
      op: "set_section_break";
      section: SecSel;
      new_page_before?: boolean;
      new_page_after?: boolean;
    }
  | { op: "set_can_grow"; object: ObjSel; can_grow: boolean }
  | {
      op: "set_suppress";
      object?: ObjSel;
      section?: SecSel;
      suppress: boolean;
      suppress_if_blank?: boolean;
    }
  | {
      op: "set_border";
      object: ObjSel;
      left?: string;
      right?: string;
      top?: string;
      bottom?: string;
      color?: string;
    }
  | {
      op: "set_number_format";
      object: ObjSel;
      decimal_places?: number;
      thousands_separator?: boolean;
      suppress_if_zero?: boolean;
    }
  | { op: "remove_object"; object: ObjSel }
  | {
      op: "add_text";
      section: SecSel;
      name: string;
      left: number;
      top: number;
      width: number;
      height: number;
      text: string;
    }
  | { op: "add_line"; section: SecSel; name: string; left: number; top: number; width: number; height: number }
  | { op: "add_box"; section: SecSel; name: string; left: number; top: number; width: number; height: number }
  | {
      op: "add_field";
      section: SecSel;
      name: string;
      field_ref: string;
      left: number;
      top: number;
      width: number;
      height: number;
    }
  | {
      op: "add_special_field";
      section: SecSel;
      name: string;
      special_type: string;
      left: number;
      top: number;
      width: number;
      height: number;
    }
  | { op: "add_group"; field_ref: string; direction?: string }
  | { op: "add_sort"; field_ref: string; direction: string }
  | { op: "set_formula"; formula: string; source: string }
  | { op: "set_record_selection"; source: string }
  | { op: "add_running_total"; name: string; summarize: string; operation: string; evaluate_field?: string; reset_field?: string }
  | {
      op: "add_subreport";
      section: SecSel;
      name: string;
      report_path: string;
      left: number;
      top: number;
      width: number;
      height: number;
    }
  | {
      op: "set_subreport_link";
      target: string;
      main_report_field: string;
      subreport_field: string;
      linked_parameter: string;
    }
  | { op: "remove_table"; alias: string }
  | { op: "add_table"; target: string; new_name: string; table_name: string }
  | { op: "set_table_location"; target: string; table_name: string }
  // --- Tier 0 extensions ---
  | { op: "set_tooltip"; object: ObjSel; text: string }
  | { op: "set_hyperlink"; object: ObjSel; kind: string; text?: string }
  | { op: "set_css_class"; object: ObjSel; css_class: string }
  | { op: "set_z_order"; object: ObjSel; z: "front" | "back" }
  | {
      op: "add_picture";
      section: SecSel;
      name: string;
      image_path: string;
      left: number;
      top: number;
      width: number;
      height: number;
    }
  | { op: "add_guideline"; axis: "horizontal" | "vertical"; position: number }
  | {
      op: "add_section";
      area: string;
      after?: string;
      name?: string;
      height?: number;
    }
  | { op: "remove_section"; area?: string; section: string }
  | {
      op: "copy_object";
      from_report: string;
      object: ObjSel;
      section: SecSel;
      new_name?: string;
    };

/** twips -> inches */
export const twipsToInches = (t: number): number => t / 1440;
/** inches -> twips */
export const inchesToTwips = (i: number): number => Math.round(i * 1440);
/** twips -> points (1/20 pt per twip) */
export const twipsToPoints = (t: number): number => t / 20;
/** points -> twips */
export const pointsToTwips = (p: number): number => Math.round(p * 20);
/** twips -> px at the given px-per-inch scale (96 is CSS default) */
export const twipsToPx = (t: number, pxPerInch = 96): number => (t / 1440) * pxPerInch;

/* ---------- VibeyReports LayoutPlan (mirrors rpt-core/src/plan.rs, camelCase) ---------- */

export interface LayoutOperation {
  action: string;
  target?: string;
  section?: string;
  newName?: string;
  leftTwips?: number;
  topTwips?: number;
  widthTwips?: number;
  heightTwips?: number;
  fontName?: string;
  fontSizePt?: number;
  bold?: boolean;
  alignment?: string;
  text?: string;
  fieldRef?: string;
  direction?: string;
  groupIndex?: number;
  sortIndex?: number;
  color?: string;
  reportPath?: string;
  mainReportField?: string;
  subreportField?: string;
  linkedParameter?: string;
  tableName?: string;
  newPageBefore?: boolean;
  newPageAfter?: boolean;
  specialType?: string;
  decimalPlaces?: number;
  thousandsSeparator?: boolean;
  suppressIfZero?: boolean;
  canGrow?: boolean;
  suppress?: boolean;
  suppressIfBlank?: boolean;
  left?: string;
  right?: string;
  top?: string;
  bottom?: string;
  // Tier 0 extensions
  kind?: string;
  z?: string;
  axis?: string;
  imagePath?: string;
  fromReport?: string;
}

export interface MarginsTwips {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

export interface NewReportOptions {
  title?: string;
  author?: string;
  comments?: string;
  orientation?: string;
  paper_size?: number;
  margins?: MarginsTwips;
}

export interface LayoutPlan {
  planVersion?: number;
  operations: LayoutOperation[];
}

/** One preview render: the PDF file bytes plus what the pipeline wants you to know. */
export interface PreviewOutput {
  /** PDF file bytes — feed to `PdfPreview` or a Blob for display/download. */
  pdf: number[];
  /** How many pages the report produced. */
  page_count: number;
  /** Pipeline fidelity warnings (unsupported object kinds, formula failures); empty = clean. */
  diagnostics: string[];
}

/** A live-database preview request: your app's ADO/OLE DB connection string, verbatim. */
export interface LiveDb {
  /** `Server=…;Database=…;User Id=…;Password=…;TrustServerCertificate=True` — parsed once, never logged. */
  connection_string: string;
  /** Report parameter values as text (`["@performance_cycle_id", "42"]`), coerced against the report's declared kinds. */
  params: [string, string][];
}
