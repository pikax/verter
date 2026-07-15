//! The script-body PARSE probe fill — the official-Acorn-parity body parse the
//! reserved `js_parse_error` slots of the official-reject gate consult.
//!
//! Owns [`script_body_fails_to_parse`]: the per-grammar OXC body reparse (plain
//! `<script>` = module JS, `lang="ts"` = TS) plus the module-scope duplicate-binding
//! early error OXC's parser defers to its binder but Acorn raises at parse. Extracted
//! from `official_reject.rs` (the file-size guard boundary); the gate's probe loop is
//! the sole consumer.

use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};

use super::expr::collect_pattern_names;
use crate::svelte::parser::ScriptBodyGrammar;

/// Whether a `<script>` body FAILS to parse the way upstream's Acorn parse does — the
/// body-probe fill. A plain `<script>` parses as JS (`SourceType::mjs()` — module JS, no TS,
/// no JSX, the Acorn-equivalent: TS-only syntax in a plain script is a parse error); a
/// `lang="ts"` body parses as TS (`SourceType::ts()`). A panic OR a non-empty parser error
/// set is a failure (`js_parse_error`).
///
/// Plus TWO parse-phase errors OXC accepts but Acorn raises, detected structurally on
/// the parsed program (typed AST, never a text heuristic):
/// - a same-scope duplicate-binding REDECLARATION (`let a; let a`, `import x; let x`,
///   `function f(){} function f(){}`, an `export`-wrapped declaration of either side)
///   — OXC's parser defers it to its binder; detected on the TOP-LEVEL binding
///   declarations, so it stays a body-slot `js_parse_error`, never a later analyze
///   fallback;
/// - a deprecated `assert { … }` import-attribute keyword in the JS grammar — Acorn
///   has no `assert` clause ("Unexpected token", oracle-probed), while OXC is
///   assert-lenient; the TS grammar KEEPS the legacy clause (official accepts it in a
///   `lang="ts"` script), so the check is Js-only.
pub(super) fn script_body_fails_to_parse(body: &str, grammar: ScriptBodyGrammar) -> bool {
    let alloc = Allocator::default();
    let source_type = match grammar {
        ScriptBodyGrammar::Js => oxc_span::SourceType::mjs(),
        ScriptBodyGrammar::Ts => oxc_span::SourceType::ts(),
    };
    let parsed = oxc_parser::Parser::new(&alloc, alloc.alloc_str(body), source_type).parse();
    if parsed.panicked || !parsed.errors.is_empty() {
        return true;
    }
    if matches!(grammar, ScriptBodyGrammar::Js) && has_assert_import_attribute(&parsed.program) {
        return true;
    }
    top_level_binding_redeclaration(&parsed.program)
}

/// Whether any import declaration carries the deprecated `assert { … }` attribute
/// keyword — an Acorn parse error in the JS grammar (`js_parse_error`) that OXC's
/// assert-lenient parser accepts. Only the `with { … }` keyword parses upstream.
fn has_assert_import_attribute(program: &Program) -> bool {
    use oxc_ast::ast::WithClauseKeyword;
    program.body.iter().any(|stmt| {
        matches!(
            stmt,
            Statement::ImportDeclaration(import)
                if import
                    .with_clause
                    .as_ref()
                    .is_some_and(|clause| clause.keyword != WithClauseKeyword::With)
        )
    })
}

/// Whether the program's TOP-LEVEL declarations contain a same-scope duplicate-binding
/// redeclaration — the module-scope early SyntaxError Acorn raises at parse ("Identifier 'x'
/// has already been declared") but OXC's parser defers to its binder.
///
/// Module top-level bindings are LEXICAL except `var`: `let` / `const` declarators, VALUE
/// import specifier locals (default / named-`as` / namespace), `function` declarations, and
/// `class` declarations all bind lexically at module scope, so ANY same-name collision
/// involving at least one lexical binding is the early error (oracle-confirmed vs the pinned
/// compiler, `js_parse_error`). An `export`-wrapped declaration and a NAMED
/// `export default function` / `class` bind the same names the bare declaration binds (an
/// anonymous default export binds nothing). `var`/`var` re-binding of the same name (legal
/// in JS) is NOT a redeclaration. Characterized by
/// `redeclaration_gate_covers_module_scope_bindings_with_official_parity`.
///
/// TS-only shapes that bind no runtime duplicate stay excluded: a type-only import
/// (decl-level `import type` or a per-specifier `type` modifier) binds no value after the TS
/// strip; a BODILESS function (a TS overload signature / `declare function`) and a `declare`
/// class are overload/ambient surface, not a parse-phase duplicate. (In the plain-JS grammar
/// a bodiless function is itself a parse error, so those exclusions are TS-reachable only.)
fn top_level_binding_redeclaration(program: &Program) -> bool {
    use oxc_ast::ast::{Declaration, ExportDefaultDeclarationKind};

    // (name, was_lexical) in source order across the top-level binding declarations. A
    // collision is a redeclaration error when EITHER the prior or the current binding is
    // lexical; two `var`s of the same name are legal.
    let mut bound: Vec<(String, bool)> = Vec::new();

    for stmt in &program.body {
        let redeclared = match stmt {
            Statement::VariableDeclaration(decl) => bind_variable_declaration(&mut bound, decl),
            Statement::ImportDeclaration(import) => bind_import_declaration(&mut bound, import),
            Statement::FunctionDeclaration(func) => bind_function(&mut bound, func),
            Statement::ClassDeclaration(class) => bind_class(&mut bound, class),
            // `export <decl>` binds the same names the bare declaration binds
            // (specifier-only `export { a }` / `export … from` re-exports bind no NEW
            // local; TS-only inner declarations bind no value).
            Statement::ExportNamedDeclaration(export) => match &export.declaration {
                Some(Declaration::VariableDeclaration(decl)) => {
                    bind_variable_declaration(&mut bound, decl)
                }
                Some(Declaration::FunctionDeclaration(func)) => bind_function(&mut bound, func),
                Some(Declaration::ClassDeclaration(class)) => bind_class(&mut bound, class),
                _ => false,
            },
            // A NAMED `export default function x() {}` / `class x {}` binds `x`; an
            // anonymous default export binds nothing.
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    bind_function(&mut bound, func)
                }
                ExportDefaultDeclarationKind::ClassDeclaration(class) => {
                    bind_class(&mut bound, class)
                }
                _ => false,
            },
            _ => false,
        };
        if redeclared {
            return true;
        }
    }
    false
}

/// Record one top-level binding; `true` when it collides with a prior binding and at
/// least one side is lexical (the Acorn early error).
fn bind_name(bound: &mut Vec<(String, bool)>, name: String, lexical: bool) -> bool {
    if let Some((_, prior_lexical)) = bound.iter().find(|(n, _)| *n == name) {
        if *prior_lexical || lexical {
            return true;
        }
    }
    bound.push((name, lexical));
    false
}

/// Bind a `var`/`let`/`const` declaration's pattern names (`let`/`const` are lexical).
/// A TS `declare` statement binds no runtime value.
fn bind_variable_declaration(
    bound: &mut Vec<(String, bool)>,
    decl: &oxc_ast::ast::VariableDeclaration,
) -> bool {
    use oxc_ast::ast::VariableDeclarationKind;
    if decl.declare {
        return false;
    }
    let lexical = matches!(
        decl.kind,
        VariableDeclarationKind::Let | VariableDeclarationKind::Const
    );
    for d in &decl.declarations {
        let mut names = Vec::new();
        collect_pattern_names(&d.id, &mut names);
        for name in names {
            if bind_name(bound, name, lexical) {
                return true;
            }
        }
    }
    false
}

/// Bind a VALUE import declaration's specifier locals (all lexical). A type-only
/// import (decl-level or per-specifier) binds no runtime value.
fn bind_import_declaration(
    bound: &mut Vec<(String, bool)>,
    import: &oxc_ast::ast::ImportDeclaration,
) -> bool {
    use oxc_ast::ast::{ImportDeclarationSpecifier, ImportOrExportKind};
    if !matches!(import.import_kind, ImportOrExportKind::Value) {
        return false;
    }
    let Some(specifiers) = &import.specifiers else {
        return false;
    };
    for spec in specifiers {
        let local = match spec {
            ImportDeclarationSpecifier::ImportSpecifier(s) => {
                if matches!(s.import_kind, ImportOrExportKind::Type) {
                    continue; // `import { type T }` — type-only member
                }
                &s.local
            }
            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
        };
        if bind_name(bound, local.name.to_string(), true) {
            return true;
        }
    }
    false
}

/// Bind a named, BODIED function declaration (lexical at module scope). A bodiless
/// function is a TS overload signature / `declare function` — no parse-phase
/// duplicate (JS-grammar bodiless never parses clean).
fn bind_function(bound: &mut Vec<(String, bool)>, func: &oxc_ast::ast::Function) -> bool {
    if func.body.is_none() {
        return false;
    }
    match &func.id {
        Some(id) => bind_name(bound, id.name.to_string(), true),
        None => false,
    }
}

/// Bind a named, non-ambient class declaration (lexical at module scope). A
/// `declare class` is TS ambient surface — no parse-phase duplicate.
fn bind_class(bound: &mut Vec<(String, bool)>, class: &oxc_ast::ast::Class) -> bool {
    if class.declare {
        return false;
    }
    match &class.id {
        Some(id) => bind_name(bound, id.name.to_string(), true),
        None => false,
    }
}
