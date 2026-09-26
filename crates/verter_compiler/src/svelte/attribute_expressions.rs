//! Typed lowering of plain-attribute `{expr}` values.
//!
//! The Svelte parser records an attribute interpolation as its inner span and
//! performs no type lowering. This producer runs once per carrier parse — while
//! the artifact is being built — reads each of those spans exactly once, and
//! retains the lowered [`IndexedValueExpression`] on the carrier payload.
//! Fact producers downstream (see [`template_facts`](super::template_facts))
//! consume the retained record; they never re-read or re-parse the value text.

use oxc_allocator::Allocator;
use oxc_span::SourceType;
use verter_span::Span;
use verter_type_expr::IndexedValueExpression;

use super::parser::{ParsedSvelte, SvelteAttributeKind, SvelteAttributeValue, SvelteNode};

/// The lowered plain-attribute values of one parsed component, keyed by the
/// interpolation's inner span and ordered by that span's start.
#[derive(Debug, Default, Clone)]
pub struct SvelteAttributeExpressions {
    records: Vec<(Span, IndexedValueExpression)>,
}

impl SvelteAttributeExpressions {
    /// Lower every plain-attribute interpolation in `parsed`.
    #[must_use]
    pub fn lower(parsed: &ParsedSvelte, source: &str) -> Self {
        let mut records = Vec::new();
        collect(&parsed.template, source, &mut records);
        records.sort_by_key(|(span, _)| span.start);
        Self { records }
    }

    /// The record retained for the value at `span`, or `None` when that value
    /// did not lower (an empty or unparsable interpolation).
    #[must_use]
    pub fn get(&self, span: Span) -> Option<&IndexedValueExpression> {
        self.records
            .binary_search_by_key(&span.start, |(recorded, _)| recorded.start)
            .ok()
            .map(|index| &self.records[index].1)
    }
}

fn collect(nodes: &[SvelteNode], source: &str, records: &mut Vec<(Span, IndexedValueExpression)>) {
    for node in nodes {
        match node {
            SvelteNode::Element(element) => {
                for attr in &element.attributes {
                    let SvelteAttributeKind::Plain {
                        value: Some(SvelteAttributeValue::Expression(span)),
                        ..
                    } = &attr.kind
                    else {
                        continue;
                    };
                    if let Some(expression) = lower(*span, source) {
                        records.push((*span, expression));
                    }
                }
                collect(&element.children, source, records);
            }
            SvelteNode::Block(block) => {
                collect(&block.children, source, records);
                for clause in &block.clauses {
                    collect(&clause.children, source, records);
                }
            }
            _ => {}
        }
    }
}

/// Lower the interpolation at `span` into typed IR at carrier coordinates.
fn lower(span: Span, source: &str) -> Option<IndexedValueExpression> {
    let raw = source.get(span.start as usize..span.end as usize)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let leading = u32::try_from(raw.len().saturating_sub(raw.trim_start().len())).ok()?;
    let allocator = Allocator::default();
    let expression = oxc_parser::Parser::new(&allocator, trimmed, SourceType::ts())
        .parse_expression()
        .ok()?;
    let mut indexed = verter_semantic::analysis::type_eval_build::lower_indexed_value_expression(
        &expression,
        trimmed,
    );
    verter_semantic::analysis::type_eval_build::offset_indexed_value_expression(
        &mut indexed,
        span.start.saturating_add(leading),
    );
    Some(indexed)
}
