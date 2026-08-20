//! Hoist a `_ctx.` read repeated within one `_renderEffect` group to a local
//! `const` — official `processRepeatedVariables`/`processRepeatedExpressions`
//! (`packages/compiler-vapor/src/generators/utils.ts`), scoped to the
//! resolved-`_ctx.<path>` case.
//!
//! Official dedups at the SOURCE-expression level, before prefixing. Verter's
//! codegen prefixes eagerly, so by the time effects reach this pass every
//! read is already `_ctx.count`-shaped text; since a given binding name
//! prefixes to the same generated text every time, exact post-prefix
//! substring identity is a safe, simpler proxy for official's source-level
//! identity check here.
//!
//! Scope: only `_ctx.<ident>(.<ident>)*` reads are considered (the common
//! non-inline case). A candidate immediately followed by `(` is skipped —
//! it's a method call, and hoisting `_ctx.fn` out from under `_ctx.fn()`
//! would change its `this` binding (official's matching
//! `isCallExpression(parentOfMemberExp)` guard). Content inside a string or
//! template literal is never scanned.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::code_transform::SegmentAnchor;
use crate::template::code_gen::types::{CodeGenOutput, VaporEffect, VaporTextPart};

/// One place a `_ctx.` read might occur: a `SetText` dynamic text part, or
/// another effect's whole `expr`. `anchors` is empty for the latter — Vapor
/// effects other than `SetText` carry no per-expression anchors today.
struct ScanTarget<'a> {
    effect_idx: usize,
    part_idx: Option<usize>,
    text: &'a str,
    anchors: &'a [SegmentAnchor],
}

struct Occurrence {
    target_idx: usize,
    start: usize,
    end: usize,
}

/// Hoist repeated `_ctx.` reads across `effects` (the set about to share one
/// `_renderEffect` block). Mutates matched text in place (bump-reallocated)
/// and returns `(decl_text, decl_anchors)` pairs to emit as `const` lines
/// before the effects — empty if nothing repeats.
pub fn hoist_repeated_ctx_reads<'alloc>(
    effects: &mut [VaporEffect<'alloc>],
    out: &mut CodeGenOutput<'alloc>,
) -> Vec<(String, Vec<SegmentAnchor>)> {
    let targets = collect_scan_targets(effects);
    if targets.is_empty() {
        return Vec::new();
    }

    let mut order: Vec<String> = Vec::new();
    let mut groups: FxHashMap<String, Vec<Occurrence>> = FxHashMap::default();
    for (ti, target) in targets.iter().enumerate() {
        for (start, end) in find_ctx_tokens(target.text) {
            let tok = target.text[start..end].to_string();
            groups.entry(tok.clone()).or_insert_with(|| {
                order.push(tok.clone());
                Vec::new()
            });
            groups
                .get_mut(&tok)
                .expect("just inserted")
                .push(Occurrence {
                    target_idx: ti,
                    start,
                    end,
                });
        }
    }

    let mut decl_list: Vec<(String, Vec<SegmentAnchor>)> = Vec::new();
    let mut used_names: FxHashSet<String> = FxHashSet::default();
    let mut plans: FxHashMap<usize, Vec<(usize, usize, String)>> = FxHashMap::default();

    for tok in &order {
        let occs = &groups[tok];
        if occs.len() < 2 {
            continue;
        }
        let var_name = gen_var_name(tok, &mut used_names);
        let decl_text = format!("const {var_name} = {tok}");

        let first = &occs[0];
        let first_target = &targets[first.target_idx];
        let prefix_len = decl_text.len() - tok.len();
        let mut decl_anchors = Vec::new();
        for a in first_target.anchors {
            let a_start = a.content_offset as usize;
            let a_end = a_start + a.length as usize;
            if a_start >= first.start && a_end <= first.end {
                let shift = prefix_len as i64 - first.start as i64;
                decl_anchors.push(SegmentAnchor {
                    content_offset: (a.content_offset as i64 + shift) as u32,
                    length: a.length,
                    source_pos: a.source_pos,
                });
            }
        }
        decl_list.push((decl_text, decl_anchors));

        for occ in occs {
            plans
                .entry(occ.target_idx)
                .or_default()
                .push((occ.start, occ.end, var_name.clone()));
        }
    }

    if decl_list.is_empty() {
        return Vec::new();
    }

    for (ti, mut repls) in plans {
        if repls.is_empty() {
            continue;
        }
        repls.sort_by_key(|r| r.0);
        let target_effect_idx = targets[ti].effect_idx;
        let target_part_idx = targets[ti].part_idx;
        let (new_text, new_anchors) =
            apply_replacements(targets[ti].text, targets[ti].anchors, &repls);
        let alloc_text = out.alloc_str(&new_text);
        let alloc_anchors = out.alloc_segment_anchors(&new_anchors);
        match (&mut effects[target_effect_idx], target_part_idx) {
            (VaporEffect::SetText { parts, .. }, Some(pi)) => {
                parts[pi] = VaporTextPart::Dynamic(alloc_text, alloc_anchors);
            }
            (VaporEffect::SetClass { expr, .. }, None)
            | (VaporEffect::SetStyle { expr, .. }, None)
            | (VaporEffect::SetProp { expr, .. }, None)
            | (VaporEffect::SetAttr { expr, .. }, None)
            | (VaporEffect::SetDomProp { expr, .. }, None)
            | (VaporEffect::SetHtml { expr, .. }, None)
            | (VaporEffect::SetDynamicProps { expr, .. }, None) => {
                *expr = alloc_text;
            }
            _ => unreachable!("scan target addressing must match the effect it was read from"),
        }
    }

    decl_list
}

fn collect_scan_targets<'a>(effects: &[VaporEffect<'a>]) -> Vec<ScanTarget<'a>> {
    let mut targets = Vec::new();
    for (ei, eff) in effects.iter().enumerate() {
        match eff {
            VaporEffect::SetText { parts, .. } => {
                for (pi, part) in parts.iter().enumerate() {
                    if let VaporTextPart::Dynamic(text, anchors) = part {
                        targets.push(ScanTarget {
                            effect_idx: ei,
                            part_idx: Some(pi),
                            text,
                            anchors,
                        });
                    }
                }
            }
            VaporEffect::SetClass { expr, .. }
            | VaporEffect::SetStyle { expr, .. }
            | VaporEffect::SetProp { expr, .. }
            | VaporEffect::SetAttr { expr, .. }
            | VaporEffect::SetDomProp { expr, .. }
            | VaporEffect::SetHtml { expr, .. }
            | VaporEffect::SetDynamicProps { expr, .. } => {
                targets.push(ScanTarget {
                    effect_idx: ei,
                    part_idx: None,
                    text: expr,
                    anchors: &[],
                });
            }
        }
    }
    targets
}

/// Maximal `_ctx.ident(.ident)*` spans in `text`, skipping string/template
/// literal content and any candidate immediately followed by `(`.
fn find_ctx_tokens(text: &str) -> Vec<(usize, usize)> {
    const LIT: &str = "_ctx.";
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut result = Vec::new();
    let mut i = 0usize;
    let mut in_str: Option<char> = None;
    while i < chars.len() {
        let c = chars[i].1;
        if let Some(q) = in_str {
            if c == '\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            in_str = Some(c);
            i += 1;
            continue;
        }
        if matches_literal(&chars, i, LIT) {
            let preceded_ok = i == 0 || !is_ident_continue(chars[i - 1].1);
            if preceded_ok {
                let start_byte = chars[i].0;
                let mut j = i + LIT.chars().count();
                let mut valid = false;
                loop {
                    if j >= chars.len() || !is_ident_start(chars[j].1) {
                        break;
                    }
                    j += 1;
                    while j < chars.len() && is_ident_continue(chars[j].1) {
                        j += 1;
                    }
                    valid = true;
                    if j < chars.len()
                        && chars[j].1 == '.'
                        && j + 1 < chars.len()
                        && is_ident_start(chars[j + 1].1)
                    {
                        j += 1;
                        continue;
                    }
                    break;
                }
                if valid {
                    let followed_by_call = j < chars.len() && chars[j].1 == '(';
                    let end_byte = if j < chars.len() {
                        chars[j].0
                    } else {
                        text.len()
                    };
                    if !followed_by_call {
                        result.push((start_byte, end_byte));
                    }
                    i = j;
                    continue;
                }
            }
        }
        i += 1;
    }
    result
}

fn matches_literal(chars: &[(usize, char)], start: usize, lit: &str) -> bool {
    for (k, lc) in (start..).zip(lit.chars()) {
        if k >= chars.len() || chars[k].1 != lc {
            return false;
        }
    }
    true
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

/// Official `genVarName` + the `_`-prefixed local name `genDeclarations`
/// assigns identifiers: strip the `_ctx.` prefix, collapse runs of non-
/// alphanumeric bytes to a single `_`, trim a trailing `_`, then prefix `_`.
/// Collisions within this hoist batch get a `_N` suffix (mirrors official's
/// `getUniqueDeclarationName`, scoped to names this pass itself mints).
fn gen_var_name(token: &str, used: &mut FxHashSet<String>) -> String {
    let raw = token.strip_prefix("_ctx.").unwrap_or(token);
    let mut cleaned = String::with_capacity(raw.len());
    let mut last_was_sep = false;
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            cleaned.push(c);
            last_was_sep = false;
        } else if !last_was_sep {
            cleaned.push('_');
            last_was_sep = true;
        }
    }
    while cleaned.ends_with('_') {
        cleaned.pop();
    }
    let base = format!("_{cleaned}");
    if used.insert(base.clone()) {
        return base;
    }
    let mut i = 1u32;
    loop {
        let candidate = format!("{base}_{i}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        i += 1;
    }
}

/// Apply non-overlapping `(start, end, replacement)` splices to `text`,
/// carrying `anchors` forward: an anchor wholly inside a replaced span is
/// dropped (its identifier text no longer exists at that spot); one wholly
/// outside every replaced span is shifted by the enclosing segment's length
/// delta.
fn apply_replacements(
    text: &str,
    anchors: &[SegmentAnchor],
    replacements: &[(usize, usize, String)],
) -> (String, Vec<SegmentAnchor>) {
    struct Seg {
        old_start: usize,
        old_end: usize,
        new_start: usize,
    }

    let mut new_text = String::with_capacity(text.len());
    let mut segs: Vec<Seg> = Vec::new();
    let mut cursor = 0usize;
    for (start, end, repl) in replacements {
        if cursor < *start {
            let new_start = new_text.len();
            new_text.push_str(&text[cursor..*start]);
            segs.push(Seg {
                old_start: cursor,
                old_end: *start,
                new_start,
            });
        }
        new_text.push_str(repl);
        cursor = *end;
    }
    if cursor < text.len() {
        let new_start = new_text.len();
        new_text.push_str(&text[cursor..]);
        segs.push(Seg {
            old_start: cursor,
            old_end: text.len(),
            new_start,
        });
    }

    let mut new_anchors = Vec::with_capacity(anchors.len());
    for a in anchors {
        let a_start = a.content_offset as usize;
        let a_end = a_start + a.length as usize;
        if let Some(seg) = segs
            .iter()
            .find(|s| a_start >= s.old_start && a_end <= s.old_end)
        {
            let shift = seg.new_start as i64 - seg.old_start as i64;
            new_anchors.push(SegmentAnchor {
                content_offset: (a.content_offset as i64 + shift) as u32,
                length: a.length,
                source_pos: a.source_pos,
            });
        }
    }

    (new_text, new_anchors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    fn make_out(alloc: &Allocator) -> CodeGenOutput<'_> {
        CodeGenOutput::new(alloc)
    }

    #[test]
    fn find_ctx_tokens_finds_maximal_chain() {
        let toks = find_ctx_tokens("_ctx.count > 1 ? _ctx.a.b : _ctx.count");
        assert_eq!(
            toks,
            vec![(0, 10), (17, 25), (28, 38)],
            "expected count, a.b, count spans"
        );
    }

    #[test]
    fn find_ctx_tokens_skips_call_target() {
        // `_ctx.fn()` — `_ctx.fn` must not be treated as a hoistable read.
        let toks = find_ctx_tokens("_ctx.fn() + _ctx.fn()");
        assert!(toks.is_empty());
    }

    #[test]
    fn find_ctx_tokens_skips_string_literal_content() {
        let toks = find_ctx_tokens(r#""_ctx.count" + _ctx.count"#);
        assert_eq!(toks, vec![(15, 25)]);
    }

    #[test]
    fn find_ctx_tokens_respects_identifier_boundary() {
        // The first `_ctx.count` is embedded in a longer identifier
        // (`x_ctx.count`) and must not be matched; the second is a clean,
        // standalone occurrence.
        let toks = find_ctx_tokens("x_ctx.count + _ctx.count");
        assert_eq!(toks, vec![(14, 24)]);
    }

    #[test]
    fn no_hoist_when_no_repeat() {
        let alloc = Allocator::default();
        let mut out = make_out(&alloc);
        let mut effects = vec![VaporEffect::SetText {
            text_ref: 0,
            parts: vec![VaporTextPart::Dynamic("_toDisplayString(_ctx.count)", &[])],
            generated: true,
        }];
        let decls = hoist_repeated_ctx_reads(&mut effects, &mut out);
        assert!(decls.is_empty());
        assert_eq!(
            effects[0].to_code(),
            "_setText(x0, _toDisplayString(_ctx.count))"
        );
    }

    #[test]
    fn hoists_repeated_read_across_text_parts() {
        let alloc = Allocator::default();
        let mut out = make_out(&alloc);
        let mut effects = vec![VaporEffect::SetText {
            text_ref: 0,
            parts: vec![
                VaporTextPart::Dynamic("_toDisplayString(_ctx.count)", &[]),
                VaporTextPart::Static("\" / \""),
                VaporTextPart::Dynamic(
                    "_toDisplayString(_ctx.count > 1 ? \"many\" : \"one\")",
                    &[],
                ),
            ],
            generated: true,
        }];
        let decls = hoist_repeated_ctx_reads(&mut effects, &mut out);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].0, "const _count = _ctx.count");
        assert_eq!(
            effects[0].to_code(),
            "_setText(x0, _toDisplayString(_count) + \" / \" + _toDisplayString(_count > 1 ? \"many\" : \"one\"))"
        );
    }

    #[test]
    fn hoists_repeated_read_across_different_effect_kinds() {
        let alloc = Allocator::default();
        let mut out = make_out(&alloc);
        let mut effects = vec![
            VaporEffect::SetProp {
                node_ref: 0,
                attr: "title",
                expr: "_ctx.label",
            },
            VaporEffect::SetAttr {
                node_ref: 0,
                attr: "aria-label",
                expr: "_ctx.label",
            },
        ];
        let decls = hoist_repeated_ctx_reads(&mut effects, &mut out);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].0, "const _label = _ctx.label");
        assert_eq!(effects[0].to_code(), "_setProp(n0, \"title\", _label)");
        assert_eq!(effects[1].to_code(), "_setAttr(n0, \"aria-label\", _label)");
    }

    #[test]
    fn declaration_anchor_rebased_from_first_occurrence() {
        let alloc = Allocator::default();
        let mut out = make_out(&alloc);
        // "_toDisplayString(_ctx.count)" — the `count` identifier's own
        // authored anchor sits at content_offset 22 (after "_toDisplayString(_ctx."), length 5.
        let anchor = SegmentAnchor {
            content_offset: 22,
            length: 5,
            source_pos: 100,
        };
        let anchors = vec![anchor];
        let mut effects = vec![VaporEffect::SetText {
            text_ref: 0,
            parts: vec![
                VaporTextPart::Dynamic(
                    "_toDisplayString(_ctx.count)",
                    Box::leak(anchors.into_boxed_slice()),
                ),
                VaporTextPart::Dynamic("_toDisplayString(_ctx.count + 1)", &[]),
            ],
            generated: true,
        }];
        let decls = hoist_repeated_ctx_reads(&mut effects, &mut out);
        assert_eq!(decls.len(), 1);
        let (text, decl_anchors) = &decls[0];
        assert_eq!(text, "const _count = _ctx.count");
        assert_eq!(decl_anchors.len(), 1);
        // "const _count = " is 15 bytes; "_ctx.count" token's `count` starts at byte 5 within it.
        assert_eq!(decl_anchors[0].content_offset, 20);
        assert_eq!(decl_anchors[0].length, 5);
        assert_eq!(decl_anchors[0].source_pos, 100);
    }
}
