//! Sound `<style> v-bind()` usage extraction for unused-binding liveness.
//!
//! The shared style syntax IR identifies trusted `v-bind()` calls in every
//! authored dialect. OXC then extracts free identifier roots from each Vue
//! expression. Syntax or expression uncertainty marks the result incomplete so
//! callers fail open and never publish a false unused-binding diagnostic.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;
use verter_css_syntax::CssDialect;

use crate::style_planner::{transform_vue_v_bind, AuthoredStyleInput, StyleRewriteOutcome};
use crate::utils::oxc::bindings::collect_expression_free_refs;

#[derive(Debug, Default, Clone)]
pub struct StyleVBindUsage {
    pub used: FxHashSet<String>,
    pub complete: bool,
}

pub fn extract_style_v_bind_usage<'a>(
    style_contents: impl IntoIterator<Item = &'a str>,
) -> StyleVBindUsage {
    extract_style_v_bind_usage_for_dialects(
        style_contents
            .into_iter()
            .map(|content| (content, CssDialect::Css)),
    )
}

pub fn extract_style_v_bind_usage_for_dialects<'a>(
    style_contents: impl IntoIterator<Item = (&'a str, CssDialect)>,
) -> StyleVBindUsage {
    let mut used = FxHashSet::default();
    let mut complete = true;

    for (css, dialect) in style_contents {
        let input =
            AuthoredStyleInput::new(css, dialect, "style-usage", "style-usage", "style-usage");
        match transform_vue_v_bind(input, "usage") {
            Ok(StyleRewriteOutcome::Unchanged { facts })
            | Ok(StyleRewriteOutcome::Rewritten { facts, .. }) => {
                for binding in facts.v_bind_vars {
                    if !collect_expr_identifier_roots(&binding.expression, &mut used) {
                        complete = false;
                    }
                }
            }
            Err(_) => complete = false,
        }
    }

    StyleVBindUsage { used, complete }
}

pub fn extract_style_v_bind_usage_for_languages<'a>(
    style_contents: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> StyleVBindUsage {
    let mut unknown_language = false;
    let inputs = style_contents
        .into_iter()
        .filter_map(|(content, language)| {
            let dialect = match language.to_ascii_lowercase().as_str() {
                "css" => CssDialect::Css,
                "scss" => CssDialect::Scss,
                "sass" => CssDialect::Sass,
                "less" => CssDialect::Less,
                "stylus" | "styl" => CssDialect::Stylus,
                _ => {
                    unknown_language = true;
                    return None;
                }
            };
            Some((content, dialect))
        });
    let mut usage = extract_style_v_bind_usage_for_dialects(inputs);
    usage.complete &= !unknown_language;
    usage
}

pub fn expression_free_roots(expr_text: &str) -> Option<Vec<String>> {
    let mut used = FxHashSet::default();
    if !collect_expr_identifier_roots(expr_text, &mut used) {
        return None;
    }
    let mut roots: Vec<String> = used.into_iter().collect();
    roots.sort_unstable();
    Some(roots)
}

fn collect_expr_identifier_roots(expr_text: &str, used: &mut FxHashSet<String>) -> bool {
    let trimmed = expr_text.trim();
    if trimmed.is_empty() {
        return true;
    }
    let alloc = Allocator::default();
    match Parser::new(&alloc, trimmed, SourceType::tsx()).parse_expression() {
        Ok(expr) => {
            for reference in collect_expression_free_refs(&expr) {
                used.insert(reference.to_string());
            }
            true
        }
        Err(_) => false,
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
        let usage = usage(".x { color: v-bind(foo); }");
        assert!(usage.complete);
        assert!(usage.used.contains("foo"));
    }

    #[test]
    fn member_expression_root_only() {
        let usage = usage(".x { color: v-bind(theme.primary); }");
        assert!(usage.complete);
        assert!(usage.used.contains("theme"));
        assert!(!usage.used.contains("primary"));
    }

    #[test]
    fn complex_expression_collects_all_identifiers() {
        let usage = usage(".x { width: v-bind(a + b); }");
        assert!(usage.complete);
        assert!(usage.used.contains("a"));
        assert!(usage.used.contains("b"));
    }

    #[test]
    fn conditional_expression_collects_branches() {
        let usage = usage(".x { color: v-bind(cond ? light : dark); }");
        assert!(usage.complete);
        assert!(usage.used.contains("cond"));
        assert!(usage.used.contains("light"));
        assert!(usage.used.contains("dark"));
    }

    #[test]
    fn quoted_expression_is_stripped_and_parsed() {
        let usage = usage(".x { color: v-bind('foo'); }");
        assert!(usage.complete);
        assert!(usage.used.contains("foo"));
    }

    #[test]
    fn malformed_unbalanced_paren_is_incomplete() {
        assert!(!usage(".x { color: v-bind(foo; }").complete);
    }

    #[test]
    fn unparseable_expression_is_incomplete() {
        assert!(!usage(".x { color: v-bind(@@@); }").complete);
    }

    #[test]
    fn v_bind_inside_comment_is_ignored() {
        let usage = usage(".x { /* v-bind(ghost) */ color: red; }");
        assert!(usage.complete);
        assert!(!usage.used.contains("ghost"));
    }

    #[test]
    fn no_v_bind_is_complete_and_empty() {
        let usage = usage(".x { color: red; }");
        assert!(usage.complete);
        assert!(usage.used.is_empty());
    }

    #[test]
    fn multiple_v_binds_union() {
        let usage = usage(".x { color: v-bind(a); } .y { color: v-bind(b.c); }");
        assert!(usage.complete);
        assert!(usage.used.contains("a"));
        assert!(usage.used.contains("b"));
    }

    #[test]
    fn computed_member_key_root_counts() {
        let usage = usage(".x { color: v-bind(obj[key]); }");
        assert!(usage.complete);
        assert!(usage.used.contains("obj"));
        assert!(usage.used.contains("key"));
    }

    #[test]
    fn global_named_binding_in_v_bind_counts() {
        let usage = usage(".x { color: v-bind(Date); }");
        assert!(usage.complete);
        assert!(usage.used.contains("Date"));
    }

    #[test]
    fn spread_in_v_bind_object_counts() {
        let usage = usage(".x { color: v-bind({ ...base }); }");
        assert!(usage.complete);
        assert!(usage.used.contains("base"));
    }

    #[test]
    fn assignment_lhs_in_v_bind_counts() {
        let usage = usage(".x { color: v-bind((obj.x = y)); }");
        assert!(usage.complete);
        assert!(usage.used.contains("obj"));
        assert!(usage.used.contains("y"));
    }

    // @ai-generated - Usage selects the shared dialect parser used by rewriting.
    #[test]
    fn dialect_aware_usage_reads_scss_through_style_ir() {
        let usage = extract_style_v_bind_usage_for_dialects([(
            "$tone: red; .x { color: v-bind(tone); }",
            CssDialect::Scss,
        )]);
        assert!(usage.complete);
        assert!(usage.used.contains("tone"));
    }
}
