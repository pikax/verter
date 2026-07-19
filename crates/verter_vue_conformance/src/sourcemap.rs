//! Source-map row extraction and canonical comparison for the Vue
//! conformance comparator.
//!
//! Generated line/column positions are WAIVED (cosmetic formatting moves
//! them); the in-contract content of a source map is the multiset of mapping
//! rows' ORIGINAL anchors — original source, original line, original column,
//! and mapped name — plus the `sources`/`names` inventories. A missing or
//! retargeted mapping row is an in-contract failure.
//!
//! Only the v3 `mappings` VLQ field is decoded; every other top-level field
//! is carried through verbatim for round-trip encoding (the discriminator
//! guard re-encodes mutated maps).

use serde_json::Value;

/// One decoded `mappings` segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRow {
    pub generated_line: u32,
    pub generated_column: i64,
    /// `(source_index, source_line, source_column)` when the segment maps to a
    /// source.
    pub source: Option<(u32, u32, u32)>,
    pub name: Option<u32>,
}

/// A decoded source-map JSON document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMapRows {
    pub sources: Vec<String>,
    pub names: Vec<String>,
    pub rows: Vec<MapRow>,
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_value(byte: u8) -> Option<i64> {
    B64.iter().position(|&b| b == byte).map(|p| p as i64)
}

fn decode_vlq(bytes: &[u8], pos: &mut usize) -> Result<i64, String> {
    let mut shift = 0u32;
    let mut value = 0i64;
    loop {
        let byte = *bytes
            .get(*pos)
            .ok_or_else(|| "truncated VLQ field".to_string())?;
        *pos += 1;
        let digit = b64_value(byte).ok_or_else(|| format!("invalid base64 byte {byte}"))?;
        value |= (digit & 31) << shift;
        if digit & 32 == 0 {
            break;
        }
        shift += 5;
        if shift > 30 {
            return Err("VLQ field too long".to_string());
        }
    }
    let negative = value & 1 == 1;
    value >>= 1;
    Ok(if negative { -value } else { value })
}

fn encode_vlq(out: &mut String, value: i64) {
    let mut vlq = (if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    }) as u64;
    loop {
        let mut digit = (vlq & 31) as u8;
        vlq >>= 5;
        if vlq > 0 {
            digit |= 32;
        }
        out.push(B64[digit as usize] as char);
        if vlq == 0 {
            break;
        }
    }
}

/// Decode the `mappings` field of a source-map JSON document.
pub fn decode_mappings(mappings: &str) -> Result<Vec<MapRow>, String> {
    let mut rows = Vec::new();
    let mut previous = [0i64; 5];
    for (line_index, line) in mappings.split(';').enumerate() {
        previous[0] = 0; // generated column resets every line
        if line.is_empty() {
            continue;
        }
        for segment in line.split(',') {
            let bytes = segment.as_bytes();
            let mut pos = 0usize;
            let mut field = 0usize;
            let mut values = [0i64; 5];
            while pos < bytes.len() && field < 5 {
                values[field] = decode_vlq(bytes, &mut pos)?;
                previous[field] += values[field];
                values[field] = previous[field];
                field += 1;
            }
            if pos != bytes.len() {
                return Err(format!("too many VLQ fields in segment {segment:?}"));
            }
            let row = MapRow {
                generated_line: line_index as u32,
                generated_column: values[0],
                source: if field >= 4 {
                    Some((values[1] as u32, values[2] as u32, values[3] as u32))
                } else {
                    None
                },
                name: if field >= 5 {
                    Some(values[4] as u32)
                } else {
                    None
                },
            };
            rows.push(row);
        }
    }
    Ok(rows)
}

/// Re-encode decoded rows into a v3 `mappings` string (guard mutations only).
pub fn encode_mappings(rows: &[MapRow]) -> String {
    let mut out = String::new();
    let mut previous = [0i64; 5];
    let mut current_line = 0u32;
    for row in rows {
        while current_line < row.generated_line {
            out.push(';');
            current_line += 1;
            previous[0] = 0;
        }
        if !out.is_empty() && !out.ends_with(';') {
            out.push(',');
        }
        let fields = match (row.source, row.name) {
            (Some((s, l, c)), Some(n)) => {
                vec![row.generated_column, s as i64, l as i64, c as i64, n as i64]
            }
            (Some((s, l, c)), None) => {
                vec![row.generated_column, s as i64, l as i64, c as i64]
            }
            (None, _) => vec![row.generated_column],
        };
        for (index, value) in fields.iter().enumerate() {
            encode_vlq(&mut out, value - previous[index]);
            previous[index] = *value;
        }
    }
    out
}

/// Parse a source-map JSON document into decoded rows + inventories.
pub fn parse_map_rows(map_json: &str) -> Result<SourceMapRows, String> {
    let value: Value =
        serde_json::from_str(map_json).map_err(|e| format!("parse source map json: {e}"))?;
    let sources = value
        .get("sources")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let names = value
        .get("names")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let mappings = value
        .get("mappings")
        .and_then(Value::as_str)
        .ok_or_else(|| "source map missing `mappings` string".to_string())?;
    let rows = decode_mappings(mappings)?;
    Ok(SourceMapRows {
        sources,
        names,
        rows,
    })
}

/// Serialize rows + inventories back into a source-map JSON document (guard
/// mutations only). Only `version`/`sources`/`names`/`mappings` are emitted.
pub fn serialize_map_rows(map: &SourceMapRows) -> String {
    serde_json::json!({
        "version": 3,
        "sources": map.sources,
        "names": map.names,
        "mappings": encode_mappings(&map.rows),
    })
    .to_string()
}

/// The canonical, comparison-relevant projection of one map: sorted multiset
/// of original-anchor rows `(source, line, column, name)`. Generated
/// positions are waived (cosmetic formatting moves them).
pub fn canonical_rows(map: &SourceMapRows) -> Vec<(String, u32, u32, Option<String>)> {
    let mut rows: Vec<(String, u32, u32, Option<String>)> = map
        .rows
        .iter()
        .filter_map(|row| {
            row.source.map(|(s, l, c)| {
                let source = map
                    .sources
                    .get(s as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("<source#{s}>"));
                let name = row
                    .name
                    .map(|n| map.names.get(n as usize).cloned().unwrap_or_default());
                (source, l, c, name)
            })
        })
        .collect();
    rows.sort();
    rows
}
