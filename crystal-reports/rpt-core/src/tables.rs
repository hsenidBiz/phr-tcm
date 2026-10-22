//! The data-source table operations — `removeTable`, `addTable`, `setTableLocation` — the
//! `QESession` half of the cross-stream tier.
//!
//! Tables live in the query-engine session stream: each `0x0002 QeConnection` carries its
//! tables as nested `0x0003 QeTable` records (name/alias/qualified name plus inline field,
//! parameter and index arrays). The stream is `QENG`-framed — a 0x26-byte header with the AES
//! IV at `[0x16..0x26)`, then AES-128-CFB under the query-engine key, then zlib — so writing it
//! means re-encrypting with the stream's own IV (the vendored `qe_cfb_encrypt`, the exact
//! inverse of the reader's decrypt).
//!
//! Offline semantics differ from the SDK's in one measured way, documented per operation: the
//! SDK contacts the database server to verify `addTable`/`setTableLocation` and to fetch real
//! column lists; an offline editor can only write the metadata, so `addTable` clones the
//! donor's connection (exactly what the SDK does) but carries **no column list** — the engine
//! populates it on the next verify/refresh.

use rpt_reader::raw::{Dialect, RecordNode, RecordTag};
use rpt_reader::{fields, fields::FieldEdit, Rpt, StreamId};

use crate::framing;

/// `0x0002` — a query-engine connection (owns the tables).
const T_QE_CONNECTION: u16 = 0x0002;
/// `0x0003` — one table of a connection.
const T_QE_TABLE: u16 = 0x0003;

/// One located QeTable: the node's address in the stream plus its container connection.
struct QeTable<'a> {
    node: &'a RecordNode,
    connection: &'a RecordNode,
    alias: String,
}

/// Walk the QESession tree, locating every `0x3` QeTable with its owning `0x2` connection.
fn tables_in<'a>(roots: &'a [RecordNode], logical: &[u8]) -> Vec<QeTable<'a>> {
    // Connections nest under a top-level `0x1 QeSession` wrapper; collect them at any depth.
    let mut connections: Vec<&RecordNode> = Vec::new();
    fn collect<'a>(nodes: &'a [RecordNode], out: &mut Vec<&'a RecordNode>) {
        for n in nodes {
            if n.rtype == T_QE_CONNECTION {
                out.push(n);
            }
            collect(&n.children, out);
        }
    }
    collect(roots, &mut connections);
    let mut out = Vec::new();
    for conn in connections {
        for table in conn.children.iter().filter(|n| n.rtype == T_QE_TABLE) {
            let alias = fields::read(table, logical, Dialect::QeSession)
                .and_then(|r| {
                    r.fields.iter().find(|f| f.name == "alias").and_then(|f| match &f.value {
                        fields::FieldValue::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                })
                .unwrap_or_default();
            out.push(QeTable { node: table, connection: conn, alias });
        }
    }
    out
}

fn find_table<'a>(roots: &'a [RecordNode], logical: &[u8], alias: &str) -> Result<QeTable<'a>, String> {
    let known: Vec<String> = tables_in(roots, logical).into_iter().map(|t| t.alias).collect();
    tables_in(roots, logical)
        .into_iter()
        .find(|t| t.alias.eq_ignore_ascii_case(alias))
        .ok_or_else(|| {
            format!(
                "no table with the alias `{alias}` is in this report's data source. Aliases present: {}",
                if known.is_empty() { "(none)".to_string() } else { known.join(", ") }
            )
        })
}

/// Re-encode the whole report with a replacement QESession **logical** stream: deflate, encrypt
/// under the stream's own IV, keep the original `QENG` header, rewrite the container stream.
fn write_qe_logical(rpt: &Rpt, new_logical: &[u8]) -> Result<Vec<u8>, String> {
    let raw = rpt
        .stream(&StreamId::QESession)
        .ok_or("no QESession stream")?
        .raw_bytes();
    if raw.len() < 0x26 || &raw[0..4] != b"QENG" {
        return Err("the QESession stream is not QENG-framed".to_string());
    }
    let iv: [u8; 16] = raw[0x16..0x26].try_into().expect("checked length");
    let deflate = miniz_oxide::deflate::compress_to_vec_zlib(new_logical, 6);
    let body = rpt_reader::qe_cfb_encrypt(&iv, &deflate);
    let mut stream_bytes = raw[..0x26].to_vec();
    stream_bytes.extend_from_slice(&body);
    rewrite_qe_stream(rpt.original_bytes(), &stream_bytes)
}

/// Replace the QESession stream's bytes in the container (the QE twin of the writer's
/// Contents splice).
fn rewrite_qe_stream(original: &[u8], new_stream: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::{Seek, SeekFrom, Write};
    let mut comp = cfb::CompoundFile::open(std::io::Cursor::new(original.to_vec()))
        .map_err(|e| format!("open the report container: {e}"))?;
    let path = comp
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_path_buf())
        .find(|p| p.file_name().map(|n| n == "QESession").unwrap_or(false))
        .ok_or("no QESession stream in the container")?;
    {
        let mut s = comp.open_stream(&path).map_err(|e| e.to_string())?;
        s.set_len(new_stream.len() as u64).map_err(|e| e.to_string())?;
        s.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        s.write_all(new_stream).map_err(|e| e.to_string())?;
        s.flush().map_err(|e| e.to_string())?;
    }
    comp.flush().map_err(|e| e.to_string())?;
    Ok(comp.into_inner().into_inner())
}

/// Patch one named field of one QESession record, via the public field-table encoder and our
/// own splice (which fixes every enclosing record's length prefix). The bytes
/// `fields::edit` produces are unmasked; the splice masks them with the record's content mask.
fn qe_patch(
    logical: &[u8],
    roots: &[RecordNode],
    node: &RecordNode,
    field: &str,
    value: &FieldEdit,
) -> Result<Vec<u8>, String> {
    let patch = fields::edit(node, logical, Dialect::QeSession, field, value)
        .map_err(|e| format!("encode `{field}`: {e}"))?;
    let chain = ancestors_of(roots, node);
    let mask = chain
        .iter()
        .fold(node.rtype as u8, |m, n| m ^ (n.rtype & 0xff) as u8);
    let masked: Vec<u8> = patch.bytes.iter().map(|b| b ^ mask).collect();
    let span = patch.field.span.start..patch.field.span.end;
    framing::splice(logical, roots, span, &masked).map_err(|e| format!("splice `{field}`: {e}"))
}

/// The chain of records enclosing `node` (found by offset), outermost first.
fn ancestors_of<'a>(roots: &'a [RecordNode], node: &RecordNode) -> Vec<&'a RecordNode> {
    fn walk<'a>(nodes: &'a [RecordNode], want: &RecordNode, chain: &mut Vec<&'a RecordNode>) -> Option<Vec<&'a RecordNode>> {
        for n in nodes {
            if std::ptr::eq(n, want) {
                return Some(chain.clone());
            }
            chain.push(n);
            if let Some(found) = walk(&n.children, want, chain) {
                return Some(found);
            }
            chain.pop();
        }
        None
    }
    walk(roots, node, &mut Vec::new()).unwrap_or_default()
}

/// `removeTable`: delete the table's `0x3` record from its connection and decrement the
/// connection's table count. The main `Contents` is untouched — placed field objects keep
/// their references and become unresolvable, exactly the SDK's measured behaviour.
pub fn remove_table(rpt: &Rpt, alias: &str) -> Result<Vec<u8>, String> {
    let stream = rpt.stream(&StreamId::QESession).ok_or("no QESession stream")?;
    let logical = stream.logical_bytes().to_vec();
    let tree = stream.record_tree();
    let table = find_table(&tree, &logical, alias)?;
    let canonical = table.alias.clone();

    // Connection count first (its header precedes the table's span, so patch-then-splice is
    // stable), then the splice.
    let count = fields::read(table.connection, &logical, Dialect::QeSession)
        .and_then(|r| {
            r.fields.iter().find(|f| f.name == "table_count").and_then(|f| match f.value {
                fields::FieldValue::Uint(v) => Some(v),
                _ => None,
            })
        })
        .ok_or("the connection's table count could not be read")?;
    let conn_ancestors: Vec<&RecordNode> = Vec::new();
    let _ = conn_ancestors;
    let mut new_logical = qe_patch(
        &logical,
        &tree,
        table.connection,
        "table_count",
        &FieldEdit::Int(i64::from(count) - 1),
    )?;
    let retree = reparse(&new_logical)?;
    let table2 = find_table(&retree, &new_logical, &canonical)?;
    new_logical = framing::splice(
        &new_logical,
        &retree,
        table2.node.offset..table2.node.content_end,
        &[],
    )
    .map_err(|e| format!("splice the table out: {e}"))?;
    let out = write_qe_logical(rpt, &new_logical)?;

    // Verify: the reopened report's tables no longer list the alias.
    let check = crate::open_bytes(&out)?;
    let tables: Vec<String> = check
        .report()
        .database
        .tables
        .iter()
        .map(|t| t.alias.clone())
        .collect();
    if tables.iter().any(|t| t.eq_ignore_ascii_case(&canonical)) {
        return Err(format!("table `{canonical}` survived its own removal"));
    }
    Ok(out)
}

/// `addTable`: clone a donor table's record under the same connection with a new identity and
/// no column list (the server fills the columns on the next verify — the offline counterpart of
/// the SDK's live column fetch).
pub fn add_table(rpt: &Rpt, donor_alias: &str, new_alias: &str, table_name: &str) -> Result<Vec<u8>, String> {
    let stream = rpt.stream(&StreamId::QESession).ok_or("no QESession stream")?;
    let logical = stream.logical_bytes().to_vec();
    let tree = stream.record_tree();

    if tables_in(&tree, &logical).iter().any(|t| t.alias.eq_ignore_ascii_case(new_alias)) {
        return Err(format!("a table with the alias `{new_alias}` is already in this report's data source"));
    }
    let donor = find_table(&tree, &logical, donor_alias)?;

    // Clone the donor record verbatim and insert it after the donor (inside the same
    // connection's content — same nesting, so no re-mask).
    let run = donor.node.offset..donor.node.content_end;
    let bytes = logical
        .get(run.clone())
        .ok_or("donor table span outside the stream")?
        .to_vec();
    let mut new_logical = framing::splice(&logical, &tree, run.end..run.end, &bytes)
        .map_err(|e| format!("splice the table in: {e}"))?;

    // Bump the connection's table count, then rebind the clone (the LAST table of the
    // connection now): identity, and strip the donor's columns/parameters/indexes.
    let mut retree = reparse(&new_logical)?;
    let mut conns: Vec<&RecordNode> = Vec::new();
    collect_connections(&retree, &mut conns);
    let conn2 = conns.first().copied().ok_or("connection vanished after insert")?;
    let count = fields::read(conn2, &new_logical, Dialect::QeSession)
        .and_then(|r| {
            r.fields.iter().find(|f| f.name == "table_count").and_then(|f| match f.value {
                fields::FieldValue::Uint(v) => Some(v),
                _ => None,
            })
        })
        .unwrap_or(0);
    new_logical = qe_patch(&new_logical, &retree, conn2, "table_count", &FieldEdit::Int(i64::from(count) + 1))?;

    retree = reparse(&new_logical)?;
    let tables = tables_in(&retree, &new_logical);
    let _clone = tables
        .last()
        .ok_or("the cloned table is not present after insert")?;
    let max_id = tables
        .iter()
        .filter_map(|t| {
            fields::read(t.node, &new_logical, Dialect::QeSession).and_then(|r| {
                r.fields.iter().find(|f| f.name == "table_id").and_then(|f| match f.value {
                    fields::FieldValue::Uint(v) => Some(v),
                    _ => None,
                })
            })
        })
        .max()
        .unwrap_or(0);

    // Rebind identity fields, reparsing between writes (each patch moves offsets).
    {
        let t = reparse(&new_logical)?;
        let c = find_last_table(&t, &new_logical)?;
        new_logical = qe_patch(&new_logical, &t, c.node, "table_id", &FieldEdit::Int(i64::from(max_id) + 1))?;
    }
    {
        let t = reparse(&new_logical)?;
        let c = find_last_table(&t, &new_logical)?;
        new_logical = qe_patch(&new_logical, &t, c.node, "name", &FieldEdit::Text(table_name.to_string()))?;
    }
    {
        let t = reparse(&new_logical)?;
        let c = find_last_table(&t, &new_logical)?;
        new_logical = qe_patch(&new_logical, &t, c.node, "alias", &FieldEdit::Text(new_alias.to_string()))?;
    }
    if let Some(qualified) = qualified_of(&donor, &logical) {
        let new_qualified = requalified(&qualified, table_name);
        let t = reparse(&new_logical)?;
        let c = find_last_table(&t, &new_logical)?;
        new_logical = qe_patch(&new_logical, &t, c.node, "qualified_name", &FieldEdit::Text(new_qualified))?;
    }

    // Strip the donor's column/parameter/index arrays: the offline table carries no schema.
    // Each iteration reparses — a strip moves every offset after it.
    for (count_field, prefix) in
        [("field_count", "fields"), ("parameter_count", "parameters"), ("index_count", "indexes")]
    {
        let tree_n = reparse(&new_logical)?;
        let table_n = find_last_table(&tree_n, &new_logical)?;
        let Some(read) = fields::read(table_n.node, &new_logical, Dialect::QeSession) else {
            continue;
        };
        let n = read
            .fields
            .iter()
            .find(|f| f.name == count_field)
            .and_then(|f| match f.value {
                fields::FieldValue::Uint(v) => Some(v),
                _ => None,
            })
            .unwrap_or(0);
        if n == 0 {
            continue;
        }
        let items: Vec<std::ops::Range<usize>> = read
            .fields
            .iter()
            .filter(|f| f.path.starts_with(&format!("{prefix}[")))
            .map(|f| f.span.start..f.span.end)
            .collect();
        if let (Some(first), Some(last)) = (items.first(), items.last()) {
            new_logical = framing::splice(&new_logical, &tree_n, first.start..last.end, &[])
                .map_err(|e| format!("strip `{prefix}`: {e}"))?;
            let tree_m = reparse(&new_logical)?;
            let table_m = find_last_table(&tree_m, &new_logical)?;
            new_logical = qe_patch(&new_logical, &tree_m, table_m.node, count_field, &FieldEdit::Int(0))?;
        }
    }

    let out = write_qe_logical(rpt, &new_logical)?;
    let check = crate::open_bytes(&out)?;
    let added = check
        .report()
        .database
        .tables
        .iter()
        .find(|t| t.alias.eq_ignore_ascii_case(new_alias));
    match added {
        Some(t) if t.data_fields.is_empty() => Ok(out),
        Some(t) => Err(format!(
            "the added table carried {} column(s) — the offline add must be schema-less",
            t.data_fields.len()
        )),
        None => Err(format!("table `{new_alias}` did not register after the add")),
    }
}

/// `setTableLocation`: repoint a table at another database object on its own connection —
/// name and qualified name only; the alias (which every placed field binds through) is left
/// alone, exactly as the SDK does.
pub fn set_table_location(rpt: &Rpt, alias: &str, table_name: &str) -> Result<Vec<u8>, String> {
    let stream = rpt.stream(&StreamId::QESession).ok_or("no QESession stream")?;
    let logical = stream.logical_bytes().to_vec();
    let tree = stream.record_tree();
    let table = find_table(&tree, &logical, alias)?;
    let qualified = qualified_of(&table, &logical);

    let mut new_logical = qe_patch(&logical, &tree, table.node, "name", &FieldEdit::Text(table_name.to_string()))?;
    if let Some(q) = qualified {
        let retree = reparse(&new_logical)?;
        let t2 = find_table(&retree, &new_logical, alias)?;
        new_logical = qe_patch(&new_logical, &retree, t2.node, "qualified_name", &FieldEdit::Text(requalified(&q, table_name)))?;
    }
    let out = write_qe_logical(rpt, &new_logical)?;
    let check = crate::open_bytes(&out)?;
    let renamed = check
        .report()
        .database
        .tables
        .iter()
        .find(|t| t.alias.eq_ignore_ascii_case(alias));
    match renamed {
        Some(t) if t.name.eq_ignore_ascii_case(table_name) => Ok(out),
        Some(t) => Err(format!("table read back as `{}`", t.name)),
        None => Err(format!("table `{alias}` vanished after the rename")),
    }
}

/* ---------------- helpers ---------------- */

fn collect_connections<'a>(nodes: &'a [RecordNode], out: &mut Vec<&'a RecordNode>) {
    for n in nodes {
        if n.rtype == T_QE_CONNECTION {
            out.push(n);
        }
        collect_connections(&n.children, out);
    }
}

fn reparse(logical: &[u8]) -> Result<Vec<RecordNode>, String> {
    let stream = rpt_reader::raw::RecordStream::from_logical_bytes(StreamId::QESession, logical);
    Ok(stream.record_tree())
}

fn find_last_table<'a>(roots: &'a [RecordNode], logical: &[u8]) -> Result<QeTable<'a>, String> {
    tables_in(roots, logical)
        .into_iter()
        .last()
        .ok_or_else(|| "no QeTable records in the stream".to_string())
}

fn qualified_of(table: &QeTable<'_>, logical: &[u8]) -> Option<String> {
    fields::read(table.node, logical, Dialect::QeSession).and_then(|r| {
        r.fields.iter().find(|f| f.name == "qualified_name").and_then(|f| match &f.value {
            fields::FieldValue::Text(t) => Some(t.clone()),
            _ => None,
        })
    })
}

/// Keep the qualified name's catalog/schema prefix, substituting the object name — the rule
/// the driving reports' own names follow (`catalog.schema.sp_x;1` ends with the name).
fn requalified(qualified: &str, table_name: &str) -> String {
    let dot = qualified.rfind('.');
    match dot {
        Some(d) if d + 1 < qualified.len() => format!("{}{}", &qualified[..=d], table_name),
        _ => table_name.to_string(),
    }
}

/// Silence the unused-tag warning for the connection constant used only in filters.
const _: RecordTag = RecordTag(T_QE_CONNECTION);
