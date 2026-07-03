//! Source parsers: the §10.4.1 row->block partition table in
//! `docs/arch/native-typeinfo-parity.md` and the live `#[ignore = "..."]`
//! site discovery in the typeinfo test sources.

use std::collections::BTreeMap;

use crate::derive::fail;

/// The first complete double-quoted literal in `s`, returned RAW (escape
/// sequences preserved as source text). Equivalent to searching with the
/// pattern `"((?:[^"\\]|\\.)*)"`: an opening quote, then any run of
/// non-quote/non-backslash characters or backslash-escaped pairs, then a
/// closing quote; an unterminated candidate falls through to the next
/// opening quote.
fn first_quoted_literal(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut search = 0usize;
    while let Some(off) = s[search..].find('"') {
        let start = search + off + 1;
        let mut i = start;
        let mut end: Option<usize> = None;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' if i + 1 < bytes.len() => i += 2,
                b'\\' => break,
                b'"' => {
                    end = Some(i);
                    break;
                }
                _ => i += 1,
            }
        }
        if let Some(end) = end {
            return Some(&s[start..end]);
        }
        search = start;
    }
    None
}

/// The first `fn <name>` occurrence in `line`. Equivalent to searching with
/// the pattern `fn\s+(\w+)`: the literal `fn`, at least one whitespace
/// character, then a run of word characters.
fn find_fn_name(line: &str) -> Option<String> {
    let mut search = 0usize;
    while let Some(off) = line[search..].find("fn") {
        let abs = search + off;
        let rest = &line[abs + 2..];
        let ws_len: usize = rest
            .chars()
            .take_while(|c| c.is_whitespace())
            .map(char::len_utf8)
            .sum();
        if ws_len > 0 {
            let name: String = rest[ws_len..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
        search = abs + 2;
    }
    None
}

/// Return `(reason, fn_name)` for every literal-string `#[ignore = "..."]`
/// site in `source` (the reason string + the next `fn NAME` within 5 lines).
pub(crate) fn extract_sites(source: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut sites = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let line = raw.trim();
        let Some(after_ignore) = line.strip_prefix("#[ignore") else {
            continue;
        };
        let rest = after_ignore.trim_start();
        if !rest.starts_with('=') || !rest.contains('"') {
            continue;
        }
        let Some(reason) = first_quoted_literal(rest) else {
            continue;
        };
        let mut fn_name: Option<String> = None;
        let upper = (i + 6).min(lines.len());
        for candidate in &lines[i + 1..upper] {
            if let Some(name) = find_fn_name(candidate) {
                fn_name = Some(name);
                break;
            }
        }
        if let Some(name) = fn_name {
            sites.push((reason.to_string(), name));
        }
    }
    sites
}

/// Parse a §10.4.1 block header line. Equivalent to matching the anchored
/// pattern `^\*\*`([A-Z0-9._]+)`\*\* \(\d+ rows?\):` and returning the
/// block-id capture.
fn parse_block_header(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("**`")?;
    let name_len = rest
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '.' || *c == '_')
        .count();
    if name_len == 0 {
        return None;
    }
    let name = &rest[..name_len];
    let rest = rest[name_len..].strip_prefix("`** (")?;
    let digit_len = rest.chars().take_while(char::is_ascii_digit).count();
    if digit_len == 0 {
        return None;
    }
    let rest = rest[digit_len..].strip_prefix(" row")?;
    let rest = rest.strip_prefix('s').unwrap_or(rest);
    rest.strip_prefix("):")?;
    Some(name)
}

/// Parse a §10.4.1 partition row line. Equivalent to matching the anchored
/// pattern `` ^- `([a-z0-9_]+\.rs)::([A-Za-z0-9_]+)` — `([A-Za-z]+)` `` and
/// returning `(file, function, capability)`.
fn parse_partition_row(line: &str) -> Option<(String, String, String)> {
    let rest = line.strip_prefix("- `")?;
    let stem_len = rest
        .chars()
        .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_')
        .count();
    if stem_len == 0 {
        return None;
    }
    let file = format!("{}.rs", &rest[..stem_len]);
    let rest = rest[stem_len..].strip_prefix(".rs::")?;
    let fn_len = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .count();
    if fn_len == 0 {
        return None;
    }
    let func = rest[..fn_len].to_string();
    let rest = rest[fn_len..].strip_prefix("` — `")?;
    let cap_len = rest.chars().take_while(char::is_ascii_alphabetic).count();
    if cap_len == 0 {
        return None;
    }
    let cap = rest[..cap_len].to_string();
    rest[cap_len..].strip_prefix('`')?;
    Some((file, func, cap))
}

/// The parsed §10.4.1 coverage table: the `(file, function) ->
/// (block_text, capability)` rows plus every `(file, function)` key that
/// appeared MORE THAN ONCE in the table (a duplicate row silently
/// overwrites its earlier occurrence in `rows`, so `run()` must reject a
/// non-empty `duplicate_keys`).
pub(crate) struct ParsedPartition {
    pub(crate) rows: BTreeMap<(String, String), (String, String)>,
    pub(crate) duplicate_keys: Vec<(String, String)>,
}

/// Parse the §10.4.1 BEGIN/END coverage table region.
pub(crate) fn parse_partition(doc_text: &str) -> ParsedPartition {
    let begin = "<!-- BEGIN U0 row→block coverage table";
    let end = "<!-- END U0 row→block coverage table";
    let (Some(bi), Some(ei)) = (doc_text.find(begin), doc_text.find(end)) else {
        fail("could not locate §10.4.1 coverage table BEGIN/END markers");
    };
    let region = &doc_text[bi..ei];
    let mut out = BTreeMap::new();
    let mut duplicate_keys: Vec<(String, String)> = Vec::new();
    let mut current_block: Option<String> = None;
    for line in region.lines() {
        let line = line.trim();
        if let Some(name) = parse_block_header(line) {
            current_block = Some(name.to_string());
            continue;
        }
        if let (Some((file, func, cap)), Some(block)) =
            (parse_partition_row(line), current_block.as_ref())
        {
            let key = (file, func);
            if out.insert(key.clone(), (block.clone(), cap)).is_some()
                && !duplicate_keys.contains(&key)
            {
                duplicate_keys.push(key);
            }
        }
    }
    ParsedPartition {
        rows: out,
        duplicate_keys,
    }
}
