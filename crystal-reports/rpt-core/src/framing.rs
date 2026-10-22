//! TSLV framing utilities for structural edits: reading a record's header from the logical
//! stream, relocating a record run between contexts, and splicing byte ranges in or out while
//! recomputing every enclosing record's length prefix.
//!
//! The framing rules come from the vendored reader (`codec/tslv.rs`, `codec/mask.rs`):
//!
//! - A record header is `flag | typeLow [| typeWord] [| schema] [| length]`, multi-byte
//!   scalars big-endian (the extended type word little-endian).
//! - Masking is a **running XOR over the record-type stack**: every byte is stored XORed with
//!   `mask`, where `mask` accumulates `rtype & 0xFF` as records open and unwind as they close.
//!   A record's header is masked with its *ancestors'* accumulated types; its content with
//!   ancestors-plus-self.
//!
//! The stack rule is what makes relocation cheap: a self-contained record run (opener through
//! end marker) internally accumulates and unwinds its own types, so the only mask difference
//! between two locations is the *outer* stack — a constant. Moving a run is a uniform XOR of
//! its bytes by `old_outer ^ new_outer`.

use rpt_reader::raw::RecordNode;

/// The mask over one record's header: the XOR of all its ancestors' types.
pub type OuterMask = u8;

/// Decode the header of the record starting at `offset` in `logical`.
///
/// `outer` is the accumulated mask of the record's ancestors (the mask in effect when the
/// reader reached this header). Returns `None` on a short or malformed header — the caller
/// only passes offsets the reader itself identified, so this is a corruption guard.
pub fn decode_header(logical: &[u8], offset: usize, outer: OuterMask) -> Option<(u16, std::ops::Range<usize>, usize)> {
    let b = [logical.get(offset)? ^ outer, logical.get(offset + 1)? ^ outer];
    let flag = b[0];
    let mut at = offset + 2;
    let rtype: u16 = if flag & 0x04 != 0 {
        let w = u16::from_le_bytes([logical.get(at)? ^ outer, logical.get(at + 1)? ^ outer]);
        at += 2;
        w
    } else {
        ((flag & 0x03) as u16) << 8 | b[1] as u16
    };
    if flag & 0x20 != 0 {
        at += 2; // schema word
    }
    let len_bytes = match (flag >> 6) & 0x03 {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => 4,
        _ => return None,
    };
    let len_range = at..at + len_bytes;
    let mut len = 0usize;
    for i in 0..len_bytes {
        len = (len << 8) | (logical.get(len_range.start + i)? ^ outer) as usize;
    }
    Some((rtype, len_range, len))
}

/// Replace `logical[range]` with `replacement` (any relative length), recomputing the length
/// prefix of every record whose content encloses `range`, and return the new logical bytes.
///
/// Enclosing headers all precede the range, so each is patched in place before the splice;
/// the length bytes are stored masked with that record's outer mask and are written back the
/// same way. A record with no length field cannot be patched and aborts the splice.
pub fn splice(
    logical: &[u8],
    roots: &[RecordNode],
    range: std::ops::Range<usize>,
    replacement: &[u8],
) -> Result<Vec<u8>, String> {
    if range.start > range.end {
        return Err(format!("splice range {} > {}", range.start, range.end));
    }
    let delta = replacement.len() as i64 - (range.end - range.start) as i64;
    let mut out = logical.to_vec();
    if delta != 0 {
        let mut enclosing: Vec<(&RecordNode, OuterMask)> = Vec::new();
        collect_enclosing(roots, &range, 0, &mut enclosing);
        for (node, outer) in enclosing {
            let (rtype, len_range, len) = decode_header(logical, node.offset, outer).ok_or_else(|| {
                format!("cannot decode header of record {:#06x} at {}", node.rtype, node.offset)
            })?;
            if len_range.is_empty() {
                return Err(format!(
                    "record {:#06x} at {} encloses the splice but carries no length field",
                    rtype, node.offset
                ));
            }
            let new_len = len as i64 + delta;
            let width = len_range.len();
            let fits = match width {
                1 => new_len <= 0xff,
                2 => new_len <= 0xffff,
                4 => (0..=u32::MAX as i64).contains(&new_len),
                _ => false,
            };
            if new_len < 0 || !fits {
                return Err(format!(
                    "record {:#06x} at {}: new length {new_len} does not fit its {width}-byte length field",
                    rtype, node.offset
                ));
            }
            let v = new_len as u64;
            for (i, slot) in len_range.enumerate() {
                let shift = 8 * (width - 1 - i);
                out[slot] = (((v >> shift) & 0xff) as u8) ^ outer;
            }
        }
    }
    out.splice(range, replacement.to_vec());
    Ok(out)
}

fn collect_enclosing<'a>(
    nodes: &'a [RecordNode],
    range: &std::ops::Range<usize>,
    outer: OuterMask,
    out: &mut Vec<(&'a RecordNode, OuterMask)>,
) {
    for node in nodes {
        let content = node.content_start..node.content_end;
        // "Encloses" must be strict at the end boundary for an insertion point: a zero-width
        // range AT a record's content_end goes after that record, not inside it. A non-empty
        // range stays subset-contained.
        let encloses = if range.is_empty() {
            content.start <= range.start && range.start < content.end
        } else {
            content.start <= range.start && range.end <= content.end
        };
        if encloses {
            out.push((node, outer));
            // The mask over this node's children adds its own type.
            let child_outer = outer ^ (node.rtype & 0xff) as u8;
            collect_enclosing(&node.children, range, child_outer, out);
        } else if range.start < node.offset {
            break;
        }
    }
}

/// Re-mask a record run relocated between contexts: XOR every byte by the difference of the
/// outer (ancestor-stack) masks. See the module docs for why a uniform XOR is exact.
pub fn remask_run(run: &[u8], from_outer: OuterMask, to_outer: OuterMask) -> Vec<u8> {
    let flip = from_outer ^ to_outer;
    if flip == 0 {
        return run.to_vec();
    }
    run.iter().map(|b| b ^ flip).collect()
}

/// Compute the outer mask for a record located at a given point of a sibling sequence: the
/// XOR of the types of the records that enclose it (its ancestor chain from the tree root).
pub fn outer_mask_of(ancestors: &[&RecordNode]) -> OuterMask {
    ancestors.iter().fold(0u8, |m, n| m ^ (n.rtype & 0xff) as u8)
}

/// The effective bracket tag of an object opener. Drawings (`0xa9`) sit inside an `0xaa`
/// wrapper whose end marker (`0xab`) closes the run, so the wrapper is the bracket for them;
/// every other opener brackets itself (its end marker is `tag + 1`).
pub fn bracket_tag(rtype: u16, parent_rtype: Option<u16>) -> u16 {
    if rtype == 0x00a9 && parent_rtype == Some(0x00aa) {
        0x00aa
    } else {
        rtype
    }
}

/// The end-marker type of a bracket: one past the bracket tag (`0xa5` → `0xa6`).
pub fn end_marker_tag(bracket: u16) -> u16 {
    bracket.wrapping_add(1)
}

/// Frame one record from its content bytes: the standard `0xF8` header (schema word, 4-byte
/// big-endian length, enhanced string format) followed by the content XOR-masked by the tag's
/// low byte — the shape every record in these streams is written with. For synthesizing small
/// records the engine's own writer would emit (a record sort, say), not for re-framing parsed
/// ones.
pub fn build_record(tag: u16, schema: u16, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 8);
    // Flag bits 1..0 carry the type's high byte, so tags >= 0x0100 use 0xF9.
    out.push(0xF8 | ((tag >> 8) as u8 & 0x03));
    out.push((tag & 0xff) as u8);
    out.extend_from_slice(&schema.to_be_bytes());
    out.extend_from_slice(&(content.len() as u32).to_be_bytes());
    let mask = (tag & 0xff) as u8;
    out.extend(content.iter().map(|b| b ^ mask));
    out
}

/// Encode an enhanced-format length-prefixed string: u32 BE count (including the trailing NUL)
/// then the bytes and the NUL.
pub fn build_string(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 5);
    out.extend_from_slice(&((s.len() + 1) as u32).to_be_bytes());
    out.extend_from_slice(s.as_bytes());
    out.push(0);
    out
}

/// Encode a narrowing u16 (1 byte under 0x80, else 2 bytes with the top bit set as marker).
pub fn build_narrowing(v: u16) -> Vec<u8> {
    if v < 0x80 {
        vec![v as u8]
    } else {
        vec![(v >> 8) as u8 | 0x80, (v & 0xff) as u8]
    }
}
