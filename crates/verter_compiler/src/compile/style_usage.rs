//! Sound `<style> v-bind()` usage extraction for unused-binding liveness.
//!
//! A `v-bind(expr)` in a `<style>` block references a `<script setup>` binding
//! (`v-bind(foo)`, `v-bind(foo.bar)`, `v-bind(a ? b : c)`, …). For the issue-#7
//! unused-binding diagnostic, style usage must be SOUND: a missed style
//! reference would demote a genuinely-used binding to a type-only read and
//! produce a false-positive TS6133.
//!
//! The host-side `style_v_bind_vars` extraction splits each expression string on
//! `.` and keeps the first segment — correct for `v-bind(foo.bar)` (root `foo`)
//! but UNSOUND for any non-member expression (`v-bind(a + b)` yields the literal
//! `"a + b"`, matching no binding, so `a`/`b` are silently dropped). This module
//! re-derives style usage from the typed AST instead: each `v-bind()` expression
//! is parsed with OXC and its free identifier roots are collected. If ANY
//! `v-bind()` expression fails to parse cleanly, style usage is marked
//! INCOMPLETE and the caller fails open (keeps every binding live).

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use crate::utils::oxc::bindings::collect_expression_free_refs;

/// The sound style `v-bind()` usage facts for one SFC.
#[derive(Debug, Default, Clone)]
pub struct StyleVBindUsage {
    /// Free identifier roots referenced by every parsed `v-bind()` expression.
    pub used: FxHashSet<String>,
    /// `true` only when EVERY discovered `v-bind()` expression parsed cleanly. A
    /// single parse failure flips this to `false` so the caller fails open
    /// (treats all bindings as style-used, emitting no false unused diagnostic).
    pub complete: bool,
}

/// Extract the sound style `v-bind()` usage facts from the raw CSS bodies of the
/// SFC's `<style>` blocks.
///
/// `style_contents` is the raw text of each `<style>` block (the `content` span
/// sliced from the SFC source). The scan finds `v-bind(...)` calls (respecting
/// CSS comments, strings, and nested parens), parses each inner expression as a
/// TS expression, and unions its free identifier roots. A v-bind with an
/// unbalanced paren or an unparseable expression marks the result incomplete.
pub fn extract_style_v_bind_usage<'a>(
    style_contents: impl IntoIterator<Item = &'a str>,
) -> StyleVBindUsage {
    let mut used = FxHashSet::default();
    let mut complete = true;

    for css in style_contents {
        for expr in iter_v_bind_expressions(css) {
            match expr {
                Some(expr_text) => {
                    if !collect_expr_identifier_roots(expr_text, &mut used) {
                        // Unparseable expression — cannot soundly know which
                        // bindings it uses.
                        complete = false;
                    }
                }
                // Malformed v-bind (unbalanced parens) — incomplete.
                None => complete = false,
            }
        }
    }

    StyleVBindUsage { used, complete }
}

/// The free identifier roots of one `v-bind()` expression, OXC-parsed —
/// the SOUND per-expression fact producers store on analyzed v-binds
/// (`AnalyzedVBind.expr_roots`). Returns `None` on a parse failure (the
/// caller records the expression's roots as UNKNOWN and fails open).
pub fn expression_free_roots(expr_text: &str) -> Option<Vec<String>> {
    let mut used = FxHashSet::default();
    if !collect_expr_identifier_roots(expr_text, &mut used) {
        return None;
    }
    let mut roots: Vec<String> = used.into_iter().collect();
    roots.sort_unstable();
    Some(roots)
}

/// Parse `expr_text` as a TS expression and union its free identifier roots into
/// `used`. Returns `true` on a clean parse, `false` on any parse error (so the
/// caller can mark usage incomplete).
fn collect_expr_identifier_roots(expr_text: &str, used: &mut FxHashSet<String>) -> bool {
    let trimmed = expr_text.trim();
    if trimmed.is_empty() {
        // An empty `v-bind()` references nothing; it is not a parse failure.
        return true;
    }
    let alloc = Allocator::default();
    let ret = Parser::new(&alloc, trimmed, SourceType::tsx()).parse_expression();
    match ret {
        Ok(expr) => {
            // Use the COMPLETE `Visit`-based free-reference collector: the partial
            // recursive-descent `collect_expression_references` silently dropped
            // global-named roots (its `is_global` filter), assignment LHS targets,
            // and whole construct families behind `_ => {}` arms — any of which
            // would demote a genuinely style-used binding to a false TS6133. The
            // complete collector records every free identifier root, so a missed
            // construct can never under-count a use.
            for r in collect_expression_free_refs(&expr) {
                used.insert(r.to_string());
            }
            true
        }
        Err(_) => false,
    }
}

/// Iterate the inner expression text of each `v-bind(...)` call in a CSS body.
///
/// Yields `Some(expr)` for a balanced `v-bind( … )` (quotes around the
/// expression are stripped, matching Vue's `v-bind('foo')` form) and `None` for
/// an unbalanced/malformed `v-bind(` with no matching close paren. The scan
/// skips CSS block comments and string literals so a `v-bind(` inside a comment
/// or string is not treated as a real binding.
fn iter_v_bind_expressions(css: &str) -> Vec<Option<&str>> {
    let bytes = css.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut i = 0;

    while i < len {
        // Skip block comments.
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(len);
            continue;
        }
        // Skip strings.
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let quote = bytes[i];
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' && i + 1 < len {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(len);
            continue;
        }
        // Match `v-bind(`.
        if i + 7 <= len && &bytes[i..i + 7] == b"v-bind(" {
            let expr_start = i + 7;
            match find_matching_paren(bytes, expr_start) {
                Some(expr_end) => {
                    let raw = css[expr_start..expr_end].trim();
                    let cleaned = strip_wrapping_quotes(raw);
                    out.push(Some(cleaned));
                    i = expr_end + 1;
                    continue;
                }
                None => {
                    out.push(None);
                    break;
                }
            }
        }
        i += 1;
    }

    out
}

/// Find the byte index of the `)` matching the `v-bind(` whose inner expression
/// starts at `expr_start`. Respects nested parens and string literals inside the
/// expression. Returns `None` when no matching close paren exists.
fn find_matching_paren(bytes: &[u8], expr_start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut depth = 1u32;
    let mut j = expr_start;
    while j < len && depth > 0 {
        match bytes[j] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[j];
                j += 1;
                while j < len && bytes[j] != quote {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
            }
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            j += 1;
        }
    }
    if depth != 0 {
        return None;
    }
    Some(j)
}

/// Strip a single layer of matching wrapping quotes (`'foo'` / `"foo"`).
fn strip_wrapping_quotes(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('\'') && s.ends_with('\'')) || (s.starts_with('"') && s.ends_with('"')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(css: &str) -> StyleVBindUsage {
        extract_style_v_bind_usage([css])
    }

    #[test]
    fn simple_identifier_v_bind() {
        let u = usage(".x { color: v-bind(foo); }");
        assert!(u.complete, "a clean v-bind is complete");
        assert!(u.used.contains("foo"));
    }

    #[test]
    fn member_expression_root_only() {
        let u = usage(".x { color: v-bind(theme.primary); }");
        assert!(u.complete);
        assert!(u.used.contains("theme"), "member root counts");
        assert!(!u.used.contains("primary"), "member property does not");
    }

    #[test]
    fn complex_expression_collects_all_identifiers() {
        // The unsound `.split('.')` host path would yield the literal `"a + b"`,
        // dropping both `a` and `b`. The sound parse collects both.
        let u = usage(".x { width: v-bind(a + b); }");
        assert!(u.complete);
        assert!(u.used.contains("a"));
        assert!(u.used.contains("b"));
    }

    #[test]
    fn conditional_expression_collects_branches() {
        let u = usage(".x { color: v-bind(cond ? light : dark); }");
        assert!(u.complete);
        assert!(u.used.contains("cond"));
        assert!(u.used.contains("light"));
        assert!(u.used.contains("dark"));
    }

    #[test]
    fn quoted_expression_is_stripped_and_parsed() {
        let u = usage(".x { color: v-bind('foo'); }");
        assert!(u.complete);
        assert!(u.used.contains("foo"));
    }

    #[test]
    fn malformed_unbalanced_paren_is_incomplete() {
        let u = usage(".x { color: v-bind(foo; }");
        assert!(!u.complete, "an unbalanced v-bind marks usage incomplete");
    }

    #[test]
    fn unparseable_expression_is_incomplete() {
        let u = usage(".x { color: v-bind(@@@); }");
        assert!(!u.complete, "an unparseable v-bind marks usage incomplete");
    }

    #[test]
    fn v_bind_inside_comment_is_ignored() {
        let u = usage(".x { /* v-bind(ghost) */ color: red; }");
        assert!(u.complete);
        assert!(!u.used.contains("ghost"), "commented v-bind is not a use");
    }

    #[test]
    fn no_v_bind_is_complete_and_empty() {
        let u = usage(".x { color: red; }");
        assert!(u.complete);
        assert!(u.used.is_empty());
    }

    #[test]
    fn multiple_v_binds_union() {
        let u = usage(".x { color: v-bind(a); } .y { color: v-bind(b.c); }");
        assert!(u.complete);
        assert!(u.used.contains("a"));
        assert!(u.used.contains("b"));
    }

    #[test]
    fn computed_member_key_root_counts() {
        // `v-bind(obj[key])` — the partial `collect_expression_references` path
        // DID handle computed members, but the global filter dropped global-named
        // roots. The complete collector must keep BOTH the object root and the
        // computed-key root, and must not apply the global filter.
        let u = usage(".x { color: v-bind(obj[key]); }");
        assert!(u.complete);
        assert!(u.used.contains("obj"), "computed member object root counts");
        assert!(u.used.contains("key"), "computed member key root counts");
    }

    #[test]
    fn global_named_binding_in_v_bind_counts() {
        // A setup binding named like a JS global used in a v-bind must be
        // collected — the global filter must NOT gate style liveness.
        let u = usage(".x { color: v-bind(Date); }");
        assert!(u.complete);
        assert!(
            u.used.contains("Date"),
            "a global-named identifier in v-bind must be recorded for liveness"
        );
    }

    #[test]
    fn spread_in_v_bind_object_counts() {
        // `v-bind({ ...base })` — a spread element. The complete collector must
        // descend into the spread argument.
        let u = usage(".x { color: v-bind({ ...base }); }");
        assert!(u.complete);
        assert!(u.used.contains("base"), "spread argument root counts");
    }

    #[test]
    fn assignment_lhs_in_v_bind_counts() {
        // `v-bind((obj.x = y))` — an assignment expression. The complete collector
        // must capture BOTH the LHS member root and the RHS. The partial
        // `collect_expression_references` only walked the RHS.
        let u = usage(".x { color: v-bind((obj.x = y)); }");
        assert!(u.complete);
        assert!(u.used.contains("obj"), "assignment LHS member root counts");
        assert!(u.used.contains("y"), "assignment RHS counts");
    }
}
