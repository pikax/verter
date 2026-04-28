//! A6: cheap shallow parse for ambient lib registration.
//!
//! Extracts top-level declared/exported names from a `.d.ts` (or `.ts`)
//! source. Used by `register_ambient_lib` to populate the per-project
//! symbol_index. Full type lowering is deferred to the session-side
//! scheduler (see `verter_session::resolver_core::ambient_resolve`).
//!
//! Recognised top-level forms:
//!
//! - Ambient declarations: `interface Foo`, `type Foo = ...`,
//!   `declare interface/type/const/let/var/class/function/enum/namespace`.
//!   Standard `.d.ts` lib files (lib.es5.d.ts) declare globals via these
//!   forms — the `interface` / `type` keywords carry an implicit `declare`
//!   in `.d.ts` mode.
//! - Module exports: `export interface/type/class/function/enum/const/let/var`.
//!   Used for ambient libs that declare a module surface (`export {}` etc.).
//!
//! Anything else (re-exports, `export *`, default exports, namespaces with
//! body) is ignored — this is by design: registration only needs the bare
//! symbol set; semantic resolution happens lazily through the scheduler.

use std::sync::Arc;

use oxc_allocator::Allocator;
use oxc_ast::ast::{Declaration, Statement};
use oxc_parser::Parser;
use oxc_span::SourceType;

/// Parse a lib's source and return its top-level declared / exported names
/// in source order. Names are deduplicated case-sensitively while preserving
/// first occurrence.
///
/// `canonical_id` is used only for the source-type heuristic — `.d.ts` /
/// `.d.mts` / `.d.cts` parse with [`SourceType::d_ts`], everything else
/// with [`SourceType::ts`].
pub fn parse_top_level_exports(canonical_id: &str, source: &str) -> Result<Vec<Arc<str>>, String> {
    let allocator = Allocator::default();
    let source_type = if canonical_id.ends_with(".d.ts")
        || canonical_id.ends_with(".d.mts")
        || canonical_id.ends_with(".d.cts")
    {
        SourceType::d_ts()
    } else {
        SourceType::ts()
    };
    let parsed = Parser::new(&allocator, source, source_type).parse();
    if parsed.panicked {
        return Err("parser panicked".to_string());
    }
    if !parsed.errors.is_empty() {
        // Lib parse failures are surfaced with the first error message.
        return Err(format!(
            "parse error: {}",
            parsed.errors[0].message.as_ref()
        ));
    }

    let mut names: Vec<Arc<str>> = Vec::new();
    let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();

    for stmt in &parsed.program.body {
        collect_from_statement(stmt, &mut names, &mut seen);
    }

    Ok(names)
}

fn collect_from_statement(
    stmt: &Statement,
    out: &mut Vec<Arc<str>>,
    seen: &mut rustc_hash::FxHashSet<Arc<str>>,
) {
    match stmt {
        Statement::TSInterfaceDeclaration(decl) => push(out, seen, decl.id.name.as_str()),
        Statement::TSTypeAliasDeclaration(decl) => push(out, seen, decl.id.name.as_str()),
        Statement::TSEnumDeclaration(decl) => push(out, seen, decl.id.name.as_str()),
        Statement::ClassDeclaration(decl) => {
            if let Some(id) = &decl.id {
                push(out, seen, id.name.as_str());
            }
        }
        Statement::FunctionDeclaration(decl) => {
            if let Some(id) = &decl.id {
                push(out, seen, id.name.as_str());
            }
        }
        Statement::VariableDeclaration(decl) => {
            for d in &decl.declarations {
                if let Some(id) = d.id.get_identifier_name() {
                    push(out, seen, id.as_str());
                }
            }
        }
        Statement::TSModuleDeclaration(decl) => {
            // `declare module "x" { ... }` and `namespace X { ... }` — only
            // record the bound name. Body contents are not traversed (lazy
            // session-side parse handles those).
            match &decl.id {
                oxc_ast::ast::TSModuleDeclarationName::Identifier(id) => {
                    push(out, seen, id.name.as_str())
                }
                oxc_ast::ast::TSModuleDeclarationName::StringLiteral(_) => {}
            }
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = &export.declaration {
                collect_from_declaration(decl, out, seen);
            }
            // Skip `specifiers` (re-exports) — they reference imported names,
            // not top-level local declarations.
        }
        Statement::ExportDefaultDeclaration(_) => {
            // Default exports have no stable name to index on — skip.
        }
        Statement::ExportAllDeclaration(_) => {
            // `export * from 'x'` — re-export, skip.
        }
        _ => {}
    }
}

fn collect_from_declaration(
    decl: &Declaration,
    out: &mut Vec<Arc<str>>,
    seen: &mut rustc_hash::FxHashSet<Arc<str>>,
) {
    match decl {
        Declaration::TSInterfaceDeclaration(decl) => push(out, seen, decl.id.name.as_str()),
        Declaration::TSTypeAliasDeclaration(decl) => push(out, seen, decl.id.name.as_str()),
        Declaration::TSEnumDeclaration(decl) => push(out, seen, decl.id.name.as_str()),
        Declaration::ClassDeclaration(decl) => {
            if let Some(id) = &decl.id {
                push(out, seen, id.name.as_str());
            }
        }
        Declaration::FunctionDeclaration(decl) => {
            if let Some(id) = &decl.id {
                push(out, seen, id.name.as_str());
            }
        }
        Declaration::VariableDeclaration(decl) => {
            for d in &decl.declarations {
                if let Some(id) = d.id.get_identifier_name() {
                    push(out, seen, id.as_str());
                }
            }
        }
        Declaration::TSModuleDeclaration(decl) => {
            if let oxc_ast::ast::TSModuleDeclarationName::Identifier(id) = &decl.id {
                push(out, seen, id.name.as_str());
            }
        }
        Declaration::TSImportEqualsDeclaration(_) => {}
        // `declare global { ... }` blocks — body is not traversed; the
        // global names declared inside surface through `.d.ts`-style
        // ambient declarations elsewhere in the source.
        Declaration::TSGlobalDeclaration(_) => {}
    }
}

fn push(out: &mut Vec<Arc<str>>, seen: &mut rustc_hash::FxHashSet<Arc<str>>, name: &str) {
    if name.is_empty() {
        return;
    }
    let arc: Arc<str> = Arc::from(name);
    if seen.insert(Arc::clone(&arc)) {
        out.push(arc);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dts_interface_and_type_alias_top_level() {
        let src = r#"
            interface Pick<T, K extends keyof T> { /* ... */ }
            type Partial<T> = { [P in keyof T]?: T[P] };
            interface Promise<T> { /* ... */ }
        "#;
        let names = parse_top_level_exports("lib.es5.d.ts", src).unwrap();
        let s: Vec<&str> = names.iter().map(|s| &**s).collect();
        assert_eq!(s, vec!["Pick", "Partial", "Promise"]);
    }

    #[test]
    fn dts_declared_const_and_function() {
        let src = r#"
            declare const Symbol: SymbolConstructor;
            declare function parseInt(s: string, radix?: number): number;
            interface SymbolConstructor {}
        "#;
        let names = parse_top_level_exports("lib.es5.d.ts", src).unwrap();
        let s: Vec<&str> = names.iter().map(|s| &**s).collect();
        // Order: Symbol (var), parseInt (function), SymbolConstructor (interface).
        assert_eq!(s, vec!["Symbol", "parseInt", "SymbolConstructor"]);
    }

    #[test]
    fn module_exports_are_extracted() {
        let src = r#"
            export interface Foo {}
            export type Bar<T> = T;
            export const X = 1;
            export function baz() {}
        "#;
        let names = parse_top_level_exports("ambient.ts", src).unwrap();
        let s: Vec<&str> = names.iter().map(|s| &**s).collect();
        assert_eq!(s, vec!["Foo", "Bar", "X", "baz"]);
    }

    #[test]
    fn duplicate_names_dedup_first_wins() {
        let src = r#"
            interface Foo {}
            interface Foo {}
            type Bar = string;
            type Bar = number;
        "#;
        let names = parse_top_level_exports("lib.foo.d.ts", src).unwrap();
        let s: Vec<&str> = names.iter().map(|s| &**s).collect();
        assert_eq!(s, vec!["Foo", "Bar"]);
    }

    #[test]
    fn export_default_and_export_star_are_ignored() {
        let src = r#"
            export default 42;
            export * from "./other";
            export interface Real {}
        "#;
        let names = parse_top_level_exports("ambient.ts", src).unwrap();
        let s: Vec<&str> = names.iter().map(|s| &**s).collect();
        assert_eq!(s, vec!["Real"]);
    }

    #[test]
    fn parser_error_returns_err() {
        let src = "interface { invalid";
        let r = parse_top_level_exports("lib.es5.d.ts", src);
        assert!(r.is_err(), "broken source MUST surface ParseFailure");
    }
}
