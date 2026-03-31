use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType};

use verter_span::Span;

use crate::analysis::types::{hash_16, ExportSignature};

/// Safely slice content by span bounds, returning empty string for out-of-range spans.
fn safe_slice(content: &str, start: u32, end: u32) -> &str {
    content
        .get(start as usize..end as usize)
        .unwrap_or_default()
}

/// Extract per-export signatures from a file's content.
/// Each export gets a name, a hash of its declaration text, and a type flag.
pub fn extract_export_signatures(
    content: &str,
    source_type: SourceType,
    allocator: &Allocator,
) -> Vec<ExportSignature> {
    let parser = Parser::new(allocator, content, source_type).with_options(ParseOptions {
        parse_regular_expression: false,
        ..ParseOptions::default()
    });
    let result = parser.parse();
    if result.panicked {
        return Vec::new();
    }

    extract_export_signatures_from_program(content, &result.program)
}

/// Extract per-export signatures from a parsed program.
/// Used by `build_script_analysis()` to avoid double-parsing.
pub(crate) fn extract_export_signatures_from_program(
    content: &str,
    program: &Program<'_>,
) -> Vec<ExportSignature> {
    // Build a map from local binding names to their declaration spans,
    // so local re-exports like `export { foo }` can hash the declaration text.
    let binding_spans = collect_binding_spans(content, program);

    let mut out = Vec::new();

    for stmt in &program.body {
        match stmt {
            Statement::ExportNamedDeclaration(decl) => {
                if let Some(ref source) = decl.source {
                    for spec in &decl.specifiers {
                        let name = spec.exported.name().to_string();
                        let local_name = spec.local.name().to_string();
                        let hash_input = format!("reexport:{}:{}", source.value, name);
                        let local_span = if spec.local.name() != spec.exported.name() {
                            Some(spec.local.span().into())
                        } else {
                            None
                        };
                        out.push(ExportSignature {
                            name,
                            declaration_hash: hash_16(hash_input.as_bytes()),
                            is_type: decl.export_kind.is_type(),
                            span: spec.exported.span().into(),
                            reexport_source: Some(source.value.to_string()),
                            reexport_local: Some(local_name),
                            local_span,
                        });
                    }
                    continue;
                }

                if !decl.specifiers.is_empty() && decl.declaration.is_none() {
                    for spec in &decl.specifiers {
                        let name = spec.exported.name().to_string();
                        let local_name = spec.local.name().to_string();
                        // Hash the declaration text if available, otherwise fall back to name-based hash
                        let hash = if let Some(decl_text) = binding_spans.get(local_name.as_str()) {
                            hash_16(decl_text.as_bytes())
                        } else {
                            let hash_input = format!("local-reexport:{}:{}", local_name, name);
                            hash_16(hash_input.as_bytes())
                        };
                        let local_span = if spec.local.name() != spec.exported.name() {
                            Some(spec.local.span().into())
                        } else {
                            None
                        };
                        out.push(ExportSignature {
                            name,
                            declaration_hash: hash,
                            is_type: decl.export_kind.is_type(),
                            span: spec.exported.span().into(),
                            reexport_source: None,
                            reexport_local: None,
                            local_span,
                        });
                    }
                    continue;
                }

                if let Some(ref declaration) = decl.declaration {
                    extract_declaration_signatures(content, declaration, &mut out);
                }
            }
            Statement::ExportDefaultDeclaration(decl) => {
                let full_span = decl.span;
                let text = safe_slice(content, full_span.start, full_span.end);
                // For named default exports, point to the declaration's identifier.
                // For anonymous ones, point to the 'default' keyword.
                let id_span = match &decl.declaration {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                        f.id.as_ref().map(|id| id.span)
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                        c.id.as_ref().map(|id| id.span)
                    }
                    _ => None,
                };
                // 'default' keyword starts at "export ".len() = 7 bytes after export start
                let default_kw_span =
                    oxc_span::Span::new(full_span.start + 7, full_span.start + 14);
                let target_span = id_span.unwrap_or(default_kw_span);
                out.push(ExportSignature {
                    name: "default".to_string(),
                    declaration_hash: hash_16(text.as_bytes()),
                    is_type: false,
                    span: target_span.into(),
                    reexport_source: None,
                    reexport_local: None,
                    local_span: None,
                });
            }
            Statement::ExportAllDeclaration(decl) => {
                let hash_input = format!("export-all:{}", decl.source.value);
                out.push(ExportSignature {
                    name: "*".to_string(),
                    declaration_hash: hash_16(hash_input.as_bytes()),
                    is_type: decl.export_kind.is_type(),
                    span: decl.span.into(),
                    reexport_source: Some(decl.source.value.to_string()),
                    reexport_local: Some("*".to_string()),
                    local_span: None,
                });
            }
            _ => {}
        }
    }

    out
}

fn extract_declaration_signatures(
    content: &str,
    declaration: &Declaration<'_>,
    out: &mut Vec<ExportSignature>,
) {
    match declaration {
        Declaration::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(name) = binding_name(&decl.id) {
                    let hash_span = decl.span;
                    let text = safe_slice(content, hash_span.start, hash_span.end);
                    let id_span = binding_id_span(&decl.id);
                    out.push(ExportSignature {
                        name,
                        declaration_hash: hash_16(text.as_bytes()),
                        is_type: false,
                        span: id_span,
                        reexport_source: None,
                        reexport_local: None,
                        local_span: None,
                    });
                }
            }
        }
        Declaration::FunctionDeclaration(func) => {
            if let Some(ref id) = func.id {
                let hash_span = func.span;
                let text = safe_slice(content, hash_span.start, hash_span.end);
                out.push(ExportSignature {
                    name: id.name.to_string(),
                    declaration_hash: hash_16(text.as_bytes()),
                    is_type: false,
                    span: id.span.into(),
                    reexport_source: None,
                    reexport_local: None,
                    local_span: None,
                });
            }
        }
        Declaration::ClassDeclaration(cls) => {
            if let Some(ref id) = cls.id {
                let hash_span = cls.span;
                let text = safe_slice(content, hash_span.start, hash_span.end);
                out.push(ExportSignature {
                    name: id.name.to_string(),
                    declaration_hash: hash_16(text.as_bytes()),
                    is_type: false,
                    span: id.span.into(),
                    reexport_source: None,
                    reexport_local: None,
                    local_span: None,
                });
            }
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            let hash_span = iface.span;
            let text = safe_slice(content, hash_span.start, hash_span.end);
            out.push(ExportSignature {
                name: iface.id.name.to_string(),
                declaration_hash: hash_16(text.as_bytes()),
                is_type: true,
                span: iface.id.span.into(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            });
        }
        Declaration::TSTypeAliasDeclaration(alias) => {
            let hash_span = alias.span;
            let text = safe_slice(content, hash_span.start, hash_span.end);
            out.push(ExportSignature {
                name: alias.id.name.to_string(),
                declaration_hash: hash_16(text.as_bytes()),
                is_type: true,
                span: alias.id.span.into(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            });
        }
        Declaration::TSEnumDeclaration(en) => {
            let hash_span = en.span;
            let text = safe_slice(content, hash_span.start, hash_span.end);
            out.push(ExportSignature {
                name: en.id.name.to_string(),
                declaration_hash: hash_16(text.as_bytes()),
                is_type: false,
                span: en.id.span.into(),
                reexport_source: None,
                reexport_local: None,
                local_span: None,
            });
        }
        _ => {}
    }
}

fn binding_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

/// Extract the identifier span from a binding pattern.
fn binding_id_span(pattern: &BindingPattern<'_>) -> Span {
    match pattern {
        BindingPattern::BindingIdentifier(id) => id.span.into(),
        _ => Span::default(),
    }
}

use std::collections::HashMap;

/// Collect a map from binding name → declaration text for all top-level declarations.
/// Used to hash local re-exports by their declaration content rather than just names.
fn collect_binding_spans<'a>(content: &'a str, program: &Program<'_>) -> HashMap<&'a str, &'a str> {
    let mut map = HashMap::new();
    for stmt in &program.body {
        match stmt {
            Statement::VariableDeclaration(var_decl) => {
                for decl in &var_decl.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &decl.id {
                        let text = safe_slice(content, decl.span.start, decl.span.end);
                        if let Some(name_text) =
                            content.get(id.span.start as usize..id.span.end as usize)
                        {
                            map.insert(name_text, text);
                        }
                    }
                }
            }
            Statement::FunctionDeclaration(func) => {
                if let Some(ref id) = func.id {
                    let text = safe_slice(content, func.span.start, func.span.end);
                    if let Some(name_text) =
                        content.get(id.span.start as usize..id.span.end as usize)
                    {
                        map.insert(name_text, text);
                    }
                }
            }
            Statement::ClassDeclaration(cls) => {
                if let Some(ref id) = cls.id {
                    let text = safe_slice(content, cls.span.start, cls.span.end);
                    if let Some(name_text) =
                        content.get(id.span.start as usize..id.span.end as usize)
                    {
                        map.insert(name_text, text);
                    }
                }
            }
            Statement::TSInterfaceDeclaration(iface) => {
                let text = safe_slice(content, iface.span.start, iface.span.end);
                if let Some(name_text) =
                    content.get(iface.id.span.start as usize..iface.id.span.end as usize)
                {
                    map.insert(name_text, text);
                }
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                let text = safe_slice(content, alias.span.start, alias.span.end);
                if let Some(name_text) =
                    content.get(alias.id.span.start as usize..alias.id.span.end as usize)
                {
                    map.insert(name_text, text);
                }
            }
            Statement::TSEnumDeclaration(en) => {
                let text = safe_slice(content, en.span.start, en.span.end);
                if let Some(name_text) =
                    content.get(en.id.span.start as usize..en.id.span.end as usize)
                {
                    map.insert(name_text, text);
                }
            }
            // Also check exported declarations
            Statement::ExportNamedDeclaration(export) => {
                if let Some(ref declaration) = export.declaration {
                    collect_declaration_binding_spans(content, declaration, &mut map);
                }
            }
            _ => {}
        }
    }
    map
}

fn collect_declaration_binding_spans<'a>(
    content: &'a str,
    declaration: &Declaration<'_>,
    map: &mut HashMap<&'a str, &'a str>,
) {
    match declaration {
        Declaration::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let BindingPattern::BindingIdentifier(id) = &decl.id {
                    let text = safe_slice(content, decl.span.start, decl.span.end);
                    if let Some(name_text) =
                        content.get(id.span.start as usize..id.span.end as usize)
                    {
                        map.insert(name_text, text);
                    }
                }
            }
        }
        Declaration::FunctionDeclaration(func) => {
            if let Some(ref id) = func.id {
                let text = safe_slice(content, func.span.start, func.span.end);
                if let Some(name_text) = content.get(id.span.start as usize..id.span.end as usize) {
                    map.insert(name_text, text);
                }
            }
        }
        Declaration::ClassDeclaration(cls) => {
            if let Some(ref id) = cls.id {
                let text = safe_slice(content, cls.span.start, cls.span.end);
                if let Some(name_text) = content.get(id.span.start as usize..id.span.end as usize) {
                    map.insert(name_text, text);
                }
            }
        }
        Declaration::TSInterfaceDeclaration(iface) => {
            let text = safe_slice(content, iface.span.start, iface.span.end);
            if let Some(name_text) =
                content.get(iface.id.span.start as usize..iface.id.span.end as usize)
            {
                map.insert(name_text, text);
            }
        }
        Declaration::TSTypeAliasDeclaration(alias) => {
            let text = safe_slice(content, alias.span.start, alias.span.end);
            if let Some(name_text) =
                content.get(alias.id.span.start as usize..alias.id.span.end as usize)
            {
                map.insert(name_text, text);
            }
        }
        Declaration::TSEnumDeclaration(en) => {
            let text = safe_slice(content, en.span.start, en.span.end);
            if let Some(name_text) = content.get(en.id.span.start as usize..en.id.span.end as usize)
            {
                map.insert(name_text, text);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_exports(code: &str) -> Vec<ExportSignature> {
        let alloc = Allocator::new();
        let source_type = SourceType::ts();
        extract_export_signatures(code, source_type, &alloc)
    }

    /// @ai-generated - Export interface
    #[test]
    fn export_interface() {
        let sigs = parse_exports("export interface MyType { foo: string; bar: number; }");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "MyType");
        assert!(sigs[0].is_type);
    }

    /// @ai-generated - Export type alias
    #[test]
    fn export_type_alias() {
        let sigs = parse_exports("export type Foo = string | number;");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "Foo");
        assert!(sigs[0].is_type);
    }

    /// @ai-generated - Export const
    #[test]
    fn export_const() {
        let sigs = parse_exports("export const CONSTANT = 42;");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "CONSTANT");
        assert!(!sigs[0].is_type);
    }

    /// @ai-generated - Export function
    #[test]
    fn export_function() {
        let sigs = parse_exports("export function helper() { return 1; }");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "helper");
        assert!(!sigs[0].is_type);
    }

    /// @ai-generated - Re-export from another module
    #[test]
    fn reexport() {
        let sigs = parse_exports("export { Foo, Bar } from './other';");
        assert_eq!(sigs.len(), 2);
        assert_eq!(sigs[0].name, "Foo");
        assert_eq!(sigs[1].name, "Bar");
    }

    /// @ai-generated - Default export
    #[test]
    fn default_export() {
        let sigs = parse_exports("export default class MyClass {}");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "default");
        assert!(!sigs[0].is_type);
    }

    /// @ai-generated - Multiple exports
    #[test]
    fn multiple_exports() {
        let code = r#"
export interface MyType { foo: string }
export const CONSTANT = 42;
export function helper() {}
"#;
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 3);
        let names: Vec<&str> = sigs.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"MyType"));
        assert!(names.contains(&"CONSTANT"));
        assert!(names.contains(&"helper"));
    }

    /// @ai-generated - Hash changes when declaration text changes
    #[test]
    fn hash_changes_on_text_change() {
        let sigs1 = parse_exports("export interface MyType { foo: string }");
        let sigs2 = parse_exports("export interface MyType { foo: string; bar: number }");
        assert_eq!(sigs1[0].name, sigs2[0].name);
        assert_ne!(sigs1[0].declaration_hash, sigs2[0].declaration_hash);
    }

    /// @ai-generated - Hash is deterministic
    #[test]
    fn hash_is_deterministic() {
        let sigs1 = parse_exports("export interface MyType { foo: string }");
        let sigs2 = parse_exports("export interface MyType { foo: string }");
        assert_eq!(sigs1[0].declaration_hash, sigs2[0].declaration_hash);
    }

    /// @ai-generated - Export enum
    #[test]
    fn export_enum() {
        let sigs = parse_exports("export enum Color { Red, Green, Blue }");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "Color");
        assert!(!sigs[0].is_type);
    }

    /// @ai-generated - Local re-export hash changes when declaration changes
    #[test]
    fn local_reexport_hash_changes_with_declaration() {
        let sigs1 = parse_exports("const foo = 1;\nexport { foo };");
        let sigs2 = parse_exports("const foo = 999;\nexport { foo };");
        assert_eq!(sigs1[0].name, "foo");
        assert_eq!(sigs2[0].name, "foo");
        assert_ne!(
            sigs1[0].declaration_hash, sigs2[0].declaration_hash,
            "local re-export hash should change when the declaration changes"
        );
    }

    /// @ai-generated - Local re-export with alias: hash changes when source declaration changes
    #[test]
    fn local_reexport_alias_hash_changes_with_declaration() {
        let sigs1 = parse_exports("const foo = 1;\nexport { foo as bar };");
        let sigs2 = parse_exports("const foo = 999;\nexport { foo as bar };");
        assert_eq!(sigs1[0].name, "bar");
        assert_eq!(sigs2[0].name, "bar");
        assert_ne!(
            sigs1[0].declaration_hash, sigs2[0].declaration_hash,
            "aliased local re-export hash should change when the declaration changes"
        );
    }

    /// @ai-generated - Type-only re-export sets is_type flag
    #[test]
    fn type_only_reexport() {
        let sigs = parse_exports("export type { Foo } from './bar';");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "Foo");
        assert!(sigs[0].is_type, "type-only re-export should set is_type");
    }

    /// @ai-generated - Export all
    #[test]
    fn export_all() {
        let sigs = parse_exports("export * from './other';");
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "*");
    }

    // ── Export span tests ────────────────────────────────────────────

    /// @ai-generated - Export function span covers identifier
    #[test]
    fn export_function_span_covers_identifier() {
        let code = "export function foo() { return 1; }";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        assert!(span.start > 0 || span.end > 0, "span should be non-zero");
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(spanned, "foo", "span should cover the function identifier");
    }

    /// @ai-generated - Export const span covers identifier
    #[test]
    fn export_const_span_covers_identifier() {
        let code = "export const bar = 42;";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(spanned, "bar", "span should cover the const identifier");
    }

    /// @ai-generated - Export class span covers identifier
    #[test]
    fn export_class_span_covers_identifier() {
        let code = "export class MyClass {}";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(spanned, "MyClass", "span should cover the class identifier");
    }

    /// @ai-generated - Export interface span covers identifier
    #[test]
    fn export_interface_span_covers_identifier() {
        let code = "export interface IFoo { x: number }";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(
            spanned, "IFoo",
            "span should cover the interface identifier"
        );
    }

    /// @ai-generated - Export type alias span covers identifier
    #[test]
    fn export_type_alias_span_covers_identifier() {
        let code = "export type Foo = string | number;";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(
            spanned, "Foo",
            "span should cover the type alias identifier"
        );
    }

    /// @ai-generated - Export enum span covers identifier
    #[test]
    fn export_enum_span_covers_identifier() {
        let code = "export enum Color { Red, Green }";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(spanned, "Color", "span should cover the enum identifier");
    }

    /// @ai-generated - Re-export span covers exported specifier
    #[test]
    fn reexport_span_covers_specifier() {
        let code = "export { Foo } from './other';";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(spanned, "Foo", "span should cover the re-export specifier");
    }

    /// @ai-generated - Default export span covers the declaration identifier or keyword
    #[test]
    fn default_export_span() {
        let code = "export default class MyClass {}";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        // For default exports with a named declaration, span covers the name
        assert_eq!(
            spanned, "MyClass",
            "span should cover the default export class name"
        );
    }

    /// @ai-generated - Default export anonymous function → span covers 'default' keyword
    #[test]
    fn default_export_anonymous_span() {
        let code = "export default function() {}";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(
            spanned, "default",
            "anonymous default export span should cover 'default' keyword"
        );
    }

    /// @ai-generated - Local re-export span covers specifier
    #[test]
    fn local_reexport_span_covers_specifier() {
        let code = "const foo = 1;\nexport { foo };";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        let spanned = &code[span.start as usize..span.end as usize];
        assert_eq!(
            spanned, "foo",
            "local re-export span should cover the specifier"
        );
    }

    /// @ai-generated - Export all span covers the star token
    #[test]
    fn export_all_span() {
        let code = "export * from './other';";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        let span = sigs[0].span;
        // Star exports don't have a meaningful identifier; span can cover the full statement or '*'
        assert!(
            span.end > span.start,
            "export all should have a non-empty span"
        );
    }

    #[test]
    fn local_span_set_for_aliased_reexport() {
        let code = "export { foo as bar } from './x'";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "bar");
        // local_span should cover "foo" (the local name)
        let local_span = sigs[0]
            .local_span
            .expect("aliased re-export should have local_span");
        assert_eq!(
            &code[local_span.start as usize..local_span.end as usize],
            "foo"
        );
        // span should cover "bar" (the exported name)
        assert_eq!(
            &code[sigs[0].span.start as usize..sigs[0].span.end as usize],
            "bar"
        );
    }

    #[test]
    fn local_span_none_for_same_name_reexport() {
        let code = "export { foo } from './x'";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "foo");
        assert!(
            sigs[0].local_span.is_none(),
            "same-name re-export should NOT have local_span"
        );
    }

    #[test]
    fn local_span_for_default_as_named() {
        let code = "export { default as Button } from './Button.vue'";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].name, "Button");
        let local_span = sigs[0]
            .local_span
            .expect("aliased default re-export should have local_span");
        assert_eq!(
            &code[local_span.start as usize..local_span.end as usize],
            "default"
        );
    }

    #[test]
    fn local_span_none_for_declaration_export() {
        let code = "export function myFn() {}";
        let sigs = parse_exports(code);
        assert_eq!(sigs.len(), 1);
        assert!(
            sigs[0].local_span.is_none(),
            "declaration export should NOT have local_span"
        );
    }
}
