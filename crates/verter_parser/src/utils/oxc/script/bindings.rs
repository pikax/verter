//! Framework-neutral script binding inventory.
//!
//! Generic import / declaration / pattern binding collection over OXC script
//! ASTs: which identifiers a program binds, in declaration order, and where.
//! Pure syntactic inventory — no framework classification, no reactivity
//! semantics, no macro awareness. Framework layers (e.g. the Vue
//! `<script setup>` binding classifier) map these spans onto their own
//! binding-kind taxonomies.

use oxc_ast::ast::{
    BindingPattern, Expression, ImportDeclaration, ImportDeclarationSpecifier, Statement,
};

use crate::common::Span;

/// Collect every identifier bound by `pattern`, in declaration order.
///
/// Walks object patterns (properties, then rest), array patterns (elements,
/// then rest), and assignment defaults (the bound left side).
pub fn collect_pattern_binding_spans(pattern: &BindingPattern<'_>, out: &mut Vec<Span>) {
    match pattern {
        BindingPattern::BindingIdentifier(ident) => {
            out.push(Span::from(ident.span));
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_pattern_binding_spans(&prop.value, out);
            }
            if let Some(rest) = &obj.rest {
                collect_pattern_binding_spans(&rest.argument, out);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_pattern_binding_spans(elem, out);
            }
            if let Some(rest) = &arr.rest {
                collect_pattern_binding_spans(&rest.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_pattern_binding_spans(&assign.left, out);
        }
    }
}

/// Collect the local binding spans a (runtime) import declaration introduces,
/// in specifier order.
///
/// Type-only imports (`import type { … }`) and per-specifier type imports
/// (`import { type Foo }`) bind no runtime identifier and are skipped.
pub fn collect_import_binding_spans(import: &ImportDeclaration<'_>, out: &mut Vec<Span>) {
    if import.import_kind.is_type() {
        return;
    }
    if let Some(specifiers) = &import.specifiers {
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    if s.import_kind.is_type() {
                        continue;
                    }
                    out.push(Span::from(s.local.span));
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    out.push(Span::from(s.local.span));
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    out.push(Span::from(s.local.span));
                }
            }
        }
    }
}

/// The identifier span a named declaration statement binds: function,
/// class, or TS enum declarations (all of which have a runtime value).
/// Type-only declarations (`type` aliases, `interface`s) bind nothing.
pub fn declaration_binding_span(stmt: &Statement<'_>) -> Option<Span> {
    match stmt {
        Statement::FunctionDeclaration(func) => func.id.as_ref().map(|id| Span::from(id.span)),
        Statement::ClassDeclaration(class) => class.id.as_ref().map(|id| Span::from(id.span)),
        Statement::TSEnumDeclaration(e) => Some(Span::from(e.id.span)),
        _ => None,
    }
}

/// The callee name of a call expression when the callee is a simple
/// identifier (`foo(…)`); `None` for member calls, parenthesized callees,
/// and every other callee shape.
pub fn callee_identifier_name(callee: &Expression<'_>) -> Option<String> {
    match callee {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    fn parse_and<R>(source: &str, f: impl FnOnce(&oxc_ast::ast::Program<'_>, &str) -> R) -> R {
        let alloc = Allocator::default();
        let ret = Parser::new(&alloc, source, SourceType::tsx()).parse();
        assert!(ret.errors.is_empty(), "parse errors: {:?}", ret.errors);
        f(&ret.program, source)
    }

    fn names(source: &str, spans: &[Span]) -> Vec<String> {
        spans
            .iter()
            .map(|s| source[s.start as usize..s.end as usize].to_string())
            .collect()
    }

    #[test]
    fn pattern_bindings_nest_in_declaration_order() {
        parse_and(
            "const { a, b: { c }, ...rest } = x; const [d, , e = 1, ...more] = y;",
            |program, source| {
                let mut all = Vec::new();
                for stmt in &program.body {
                    if let Statement::VariableDeclaration(decl) = stmt {
                        for d in &decl.declarations {
                            collect_pattern_binding_spans(&d.id, &mut all);
                        }
                    }
                }
                assert_eq!(
                    names(source, &all),
                    vec!["a", "c", "rest", "d", "e", "more"]
                );
            },
        );
    }

    #[test]
    fn import_bindings_skip_type_only() {
        parse_and(
            "import Def from './a';
import { v, type T } from './b';
import type { U } from './c';
import * as ns from './d';",
            |program, source| {
                let mut all = Vec::new();
                for stmt in &program.body {
                    if let Statement::ImportDeclaration(import) = stmt {
                        collect_import_binding_spans(import, &mut all);
                    }
                }
                let got = names(source, &all);
                assert_eq!(got, vec!["Def", "v", "ns"]);
                assert!(!got.contains(&"T".to_string()));
                assert!(!got.contains(&"U".to_string()));
            },
        );
    }

    #[test]
    fn declaration_bindings_cover_fn_class_enum_only() {
        parse_and(
            "function f() {}
class K {}
enum E { A }
type T = string;
interface I { x: number }
const c = 1;",
            |program, source| {
                let spans: Vec<Span> = program
                    .body
                    .iter()
                    .filter_map(declaration_binding_span)
                    .collect();
                assert_eq!(names(source, &spans), vec!["f", "K", "E"]);
            },
        );
    }
}
