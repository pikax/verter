//! TypeScript angle-bracket assertion normalization for TSX IDE carriers.
//!
//! TypeScript accepts `<Type>value`, while TSX parses the same bytes as JSX.
//! Framework scripts authored with the TypeScript grammar therefore normalize
//! every `TSTypeAssertion` to `(value as Type)` before entering a TSX carrier.

use oxc_allocator::Allocator;
use oxc_ast::ast::Expression;
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use crate::code_transform::CodeTransform;

#[derive(Debug, Clone, Copy)]
struct TypeAssertionEdit {
    assertion_start: u32,
    expression_start: u32,
    assertion_end: u32,
    type_start: u32,
    type_end: u32,
}

#[derive(Default)]
struct TypeAssertionCollector {
    edits: Vec<TypeAssertionEdit>,
}

impl<'a> Visit<'a> for TypeAssertionCollector {
    fn visit_expression(&mut self, expression: &Expression<'a>) {
        // Post-order is required for nested assertions that share an end
        // boundary: the inner ` as T)` suffix must be emitted before the outer.
        walk::walk_expression(self, expression);
        if let Expression::TSTypeAssertion(assertion) = expression {
            let type_span = assertion.type_annotation.span();
            self.edits.push(TypeAssertionEdit {
                assertion_start: assertion.span.start,
                expression_start: assertion.expression.span().start,
                assertion_end: assertion.span.end,
                type_start: type_span.start,
                type_end: type_span.end,
            });
        }
    }
}

/// Rewrite every `<Type>value` assertion in an authored TypeScript body to
/// `(value as Type)` for TSX validity.
///
/// The full AST visitor covers assertions in functions, classes, exports,
/// control-flow constructs, and nested expressions. Only the assertion prefix
/// punctuation is removed; the value expression remains source-owned and the
/// authored type span is moved, rather than copied, so both retain exact source
/// identity. Only the replacement `(`, ` as `, and `)` bytes are unmapped IDE
/// scaffolding.
pub(crate) fn rewrite_ts_type_assertions(
    content: &str,
    content_start: u32,
    out: &mut CodeTransform<'_>,
) {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, content, SourceType::ts()).parse();
    let mut collector = TypeAssertionCollector::default();
    collector.visit_program(&parsed.program);

    for edit in collector.edits {
        let Some(closing_angle) = type_assertion_closing_angle(content, edit) else {
            continue;
        };
        let assertion_start = content_start + edit.assertion_start;
        let expression_start = content_start + edit.expression_start;
        let assertion_end = content_start + edit.assertion_end;
        let type_start = content_start + edit.type_start;
        let type_end = content_start + edit.type_end;
        let closing_angle = content_start + closing_angle;

        out.move_wrapped(type_start, type_end, assertion_end, " as ", ")");
        out.remove(assertion_start, assertion_start + 1);
        out.remove(closing_angle, closing_angle + 1);
        out.prepend_left(expression_start, "(");
    }
}

/// Locate the assertion's closing `>` without mistaking `>` bytes in trivia
/// for syntax. The parser owns the type and expression spans; this small lexer
/// only bridges the trivia gap between those two authoritative boundaries.
fn type_assertion_closing_angle(content: &str, edit: TypeAssertionEdit) -> Option<u32> {
    let bytes = content.as_bytes();
    let mut cursor = edit.type_end as usize;
    let expression_start = edit.expression_start as usize;

    while cursor < expression_start {
        match bytes[cursor] {
            byte if byte.is_ascii_whitespace() => cursor += 1,
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor += 2;
                while cursor < expression_start && !matches!(bytes[cursor], b'\n' | b'\r') {
                    cursor += 1;
                }
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor += 2;
                while cursor + 1 < expression_start
                    && !(bytes[cursor] == b'*' && bytes[cursor + 1] == b'/')
                {
                    cursor += 1;
                }
                if cursor + 1 >= expression_start {
                    return None;
                }
                cursor += 2;
            }
            b'>' => return Some(cursor as u32),
            _ => return None,
        }
    }

    None
}
