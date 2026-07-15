//! Shared semantic-comment oracle for emitted Svelte client modules.
//!
//! This is test/conformance infrastructure, but it deliberately lives in one
//! Rust module: live pinned-compiler extraction, committed-golden validation,
//! and raw Verter-vs-official comparison must never fork comment
//! classification or anchor semantics.

use std::collections::HashMap;

use oxc_ast::{
    ast::{Comment, CommentContent, CommentPosition, Expression, Statement},
    AstKind,
};
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;

/// A raw emitted module could not be parsed as JavaScript, so no semantic
/// comment oracle can be produced safely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticCommentSignatureError {
    /// Number of parser diagnostics produced for the invalid module.
    pub diagnostic_count: usize,
}

/// Parse one emitted JavaScript module and derive its exact semantic-comment
/// signature. Cosmetic comments are omitted; tool-consumed comments retain
/// class, bytes, occurrence-path anchor, and per-anchor order.
pub fn semantic_comment_signature(
    code: &str,
) -> Result<Vec<String>, SemanticCommentSignatureError> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, code, oxc_span::SourceType::mjs()).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(SemanticCommentSignatureError {
            diagnostic_count: parsed.errors.len().max(usize::from(parsed.panicked)),
        });
    }
    Ok(semantic_comment_signature_from_program(
        code,
        &parsed.program,
    ))
}

/// Derive the signature from an already-parsed program. The structural client
/// comparator uses this entry so its structural and comment signatures come
/// from the same OXC parse; the golden extractor calls the parsing wrapper.
#[doc(hidden)]
pub fn semantic_comment_signature_from_program(
    code: &str,
    program: &oxc_ast::ast::Program<'_>,
) -> Vec<String> {
    let mut ordered: Vec<&Comment> = program.comments.iter().collect();
    ordered.sort_by_key(|comment| (comment.span.start, comment.span.end));
    let anchors = CommentAnchorIndex::build(program);
    let mut ord_at_anchor: HashMap<String, usize> = HashMap::new();
    let mut entries = Vec::new();

    for comment in ordered {
        let raw = &code[comment.span.start as usize..comment.span.end as usize];
        let Some(class) = semantic_comment_class(comment, raw) else {
            continue;
        };
        let anchor = anchors.anchor_for(comment.attached_to, comment.span.start, comment.position);
        let ordinal = ord_at_anchor.entry(anchor.clone()).or_default();
        let ord = *ordinal;
        *ordinal += 1;
        let text = raw.replace("\r\n", "\n").replace('\r', "\n");
        entries.push(format!(
            "Comment(class={class},text={text:?},anchor={anchor:?},ord={ord})"
        ));
    }

    entries.sort();
    entries
}

fn semantic_comment_class(comment: &Comment, raw: &str) -> Option<&'static str> {
    match comment.content {
        CommentContent::Pure => return Some("Pure"),
        CommentContent::PureNotApplied => return Some("PureNotApplied"),
        CommentContent::NoSideEffects => return Some("NoSideEffects"),
        CommentContent::Legal => return Some("Legal"),
        CommentContent::JsdocLegal => return Some("JsdocLegal"),
        CommentContent::Jsdoc => return Some("Jsdoc"),
        CommentContent::Webpack => return Some("Webpack"),
        CommentContent::Vite => return Some("Vite"),
        CommentContent::Turbopack => return Some("Turbopack"),
        CommentContent::CoverageIgnore => return Some("CoverageIgnore"),
        CommentContent::None => {}
    }

    let inner = if let Some(rest) = raw.strip_prefix("/*") {
        rest.strip_suffix("*/").unwrap_or(rest)
    } else if let Some(rest) = raw.strip_prefix("//") {
        rest
    } else {
        raw
    };
    let trimmed = inner.trim_start();

    for sigil in ['#', '@'] {
        if let Some(rest) = trimmed.strip_prefix(sigil) {
            let rest = rest.trim_start();
            if rest.starts_with("sourceURL=") || rest.starts_with("sourceMappingURL=") {
                return Some("SourceMap");
            }
        }
    }

    if let Some(rest) = raw.strip_prefix("///") {
        if let Some(rest) = rest.trim_start().strip_prefix("<reference") {
            if reference_boundary(rest) {
                return Some("TsTripleSlash");
            }
        }
    }

    if raw.starts_with("//") {
        for directive in ["@ts-check", "@ts-nocheck"] {
            if let Some(rest) = trimmed.strip_prefix(directive) {
                if ts_pragma_boundary(rest) {
                    return Some("TsDirective");
                }
            }
        }
    }
    for directive in ["@ts-ignore", "@ts-expect-error"] {
        if let Some(rest) = trimmed.strip_prefix(directive) {
            if directive_boundary(rest) {
                return Some("TsDirective");
            }
        }
    }
    None
}

fn directive_boundary(rest: &str) -> bool {
    rest.chars().next().is_none_or(|character| {
        !(character.is_ascii_alphanumeric()
            || character == '_'
            || character == '$'
            || character == '-')
    })
}

fn ts_pragma_boundary(rest: &str) -> bool {
    rest.chars()
        .next()
        .is_none_or(|character| character.is_ascii_whitespace() || character == ':')
}

fn reference_boundary(rest: &str) -> bool {
    rest.chars()
        .next()
        .is_none_or(|character| character.is_whitespace() || character == '/' || character == '>')
}

fn unwrap_parens<'a, 'b>(expression: &'b Expression<'a>) -> &'b Expression<'a> {
    let mut current = expression;
    while let Expression::ParenthesizedExpression(paren) = current {
        current = &paren.expression;
    }
    current
}

#[derive(Clone)]
struct AnchorCandidate {
    start: u32,
    end: u32,
    depth: usize,
    path: String,
}

#[derive(Clone, Copy)]
enum TopSegmentKind {
    Directive,
    Statement,
}

/// Generic child-span index over the same normalized AST view as the client
/// structural comparator: list-context empty statements are transparent but
/// retain synthetic comment carriers, and redundant parentheses are
/// transparent in both leading and trailing anchor resolution.
struct CommentAnchorIndex {
    candidates: Vec<AnchorCandidate>,
    paren_alias: HashMap<u32, u32>,
    path_stack: Vec<String>,
    child_counter: Vec<u32>,
    top_index: usize,
    top_segment_kind: TopSegmentKind,
}

impl CommentAnchorIndex {
    fn build(program: &oxc_ast::ast::Program<'_>) -> Self {
        let mut index = Self {
            candidates: Vec::new(),
            paren_alias: HashMap::new(),
            path_stack: Vec::new(),
            child_counter: Vec::new(),
            top_index: 0,
            top_segment_kind: TopSegmentKind::Statement,
        };
        for (position, directive) in program.directives.iter().enumerate() {
            index.top_index = position;
            index.top_segment_kind = TopSegmentKind::Directive;
            index.visit_directive(directive);
        }
        index.top_segment_kind = TopSegmentKind::Statement;
        index.normalize_statement_list(&program.body);
        index
    }

    fn record(&mut self, span: oxc_span::Span) {
        self.candidates.push(AnchorCandidate {
            start: span.start,
            end: span.end,
            depth: self.path_stack.len(),
            path: self.path_stack.join("/"),
        });
    }

    fn record_empty_gap(&mut self, span: oxc_span::Span, logical: usize, ordinal: usize) {
        self.path_stack
            .push(format!("empty_gap[{logical}.{ordinal}]:EmptyStatement"));
        self.record(span);
        self.path_stack.pop();
    }

    fn normalize_statement_list<'a>(&mut self, statements: &oxc_allocator::Vec<'a, Statement<'a>>) {
        let mut logical = 0usize;
        let mut empty_ordinal = 0usize;
        for statement in statements {
            if let Statement::EmptyStatement(empty) = statement {
                self.record_empty_gap(empty.span, logical, empty_ordinal);
                empty_ordinal += 1;
                continue;
            }
            self.top_index = logical;
            self.visit_statement(statement);
            logical += 1;
            empty_ordinal = 0;
        }
    }

    fn anchor_for(
        &self,
        attached_to: u32,
        comment_start: u32,
        position: CommentPosition,
    ) -> String {
        let (position_name, candidate) = match position {
            CommentPosition::Leading => {
                let target = self
                    .paren_alias
                    .get(&attached_to)
                    .copied()
                    .unwrap_or(attached_to);
                let best = self
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.start == target)
                    .max_by(|left, right| {
                        (left.depth, left.end, &left.path).cmp(&(
                            right.depth,
                            right.end,
                            &right.path,
                        ))
                    });
                ("Leading", best)
            }
            CommentPosition::Trailing => {
                let best = self
                    .candidates
                    .iter()
                    .filter(|candidate| candidate.end <= comment_start)
                    .max_by(|left, right| {
                        (left.end, left.depth, &left.path).cmp(&(
                            right.end,
                            right.depth,
                            &right.path,
                        ))
                    });
                ("Trailing", best)
            }
        };
        format!(
            "pos={position_name}/{}",
            candidate.map_or("<tail>", |candidate| candidate.path.as_str())
        )
    }
}

impl<'a> Visit<'a> for CommentAnchorIndex {
    fn enter_node(&mut self, kind: AstKind<'a>) {
        let segment = if self.path_stack.is_empty() {
            match self.top_segment_kind {
                TopSegmentKind::Directive => format!("dir[{}]", self.top_index),
                TopSegmentKind::Statement => {
                    format!("stmt[{}]:{:?}", self.top_index, kind.ty())
                }
            }
        } else {
            let counter = self
                .child_counter
                .last_mut()
                .expect("an entered child always has a parent frame");
            let child = *counter;
            *counter += 1;
            format!("child[{child}]:{:?}", kind.ty())
        };
        self.path_stack.push(segment);
        self.child_counter.push(0);
        self.record(kind.span());
    }

    fn leave_node(&mut self, _kind: AstKind<'a>) {
        self.path_stack.pop();
        self.child_counter.pop();
    }

    fn visit_parenthesized_expression(
        &mut self,
        paren: &oxc_ast::ast::ParenthesizedExpression<'a>,
    ) {
        let inner = unwrap_parens(&paren.expression);
        let inner_end = inner.span().end;
        self.paren_alias
            .insert(paren.span.start, inner.span().start);
        let first_new_candidate = self.candidates.len();
        self.visit_expression(&paren.expression);
        let trailing_aliases: Vec<AnchorCandidate> = self.candidates[first_new_candidate..]
            .iter()
            .filter(|candidate| candidate.end == inner_end)
            .map(|candidate| AnchorCandidate {
                start: candidate.start,
                end: paren.span.end,
                depth: candidate.depth,
                path: candidate.path.clone(),
            })
            .collect();
        self.candidates.extend(trailing_aliases);
    }

    fn visit_statements(&mut self, statements: &oxc_allocator::Vec<'a, Statement<'a>>) {
        self.normalize_statement_list(statements);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_comment_signature_is_non_empty_and_position_sensitive() {
        let first = semantic_comment_signature("/*! keep */ f(); f();").expect("valid module");
        let second = semantic_comment_signature("f(); /*! keep */ f();").expect("valid module");
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_ne!(first, second);
    }

    #[test]
    fn semantic_comment_signature_waives_cosmetic_comments() {
        assert_eq!(
            semantic_comment_signature("/* note */ f();").expect("valid module"),
            Vec::<String>::new()
        );
    }
}
