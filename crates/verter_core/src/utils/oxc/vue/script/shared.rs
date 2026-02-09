//! Shared script parsing logic for imports and exports.
//!
//! This module handles parsing of import and export declarations,
//! which is common to both Options API and Script Setup modes.

#![allow(dead_code)]
#![allow(unused_imports)]

use oxc_ast::ast::*;
use oxc_span::GetSpan;

use super::types::{ScriptBinding, ScriptExport, ScriptImport, ScriptItem};
use crate::common::Span;

/// Context for script parsing with offset adjustment
pub struct ScriptParseContext<'a> {
    /// Base offset to add to all OXC spans
    /// This is the position where script content starts in the SFC
    pub base_offset: u32,
    /// Source bytes for byte comparisons
    pub source_bytes: &'a [u8],
}

impl<'a> ScriptParseContext<'a> {
    /// Create a new parse context
    pub fn new(base_offset: u32, source_bytes: &'a [u8]) -> Self {
        Self {
            base_offset,
            source_bytes,
        }
    }

    /// Convert an OXC span to an SFC-relative span
    #[inline]
    pub fn adjust_span(&self, oxc_span: oxc_span::Span) -> Span {
        Span::new(
            oxc_span.start + self.base_offset,
            oxc_span.end + self.base_offset,
        )
    }

    /// Get a slice of the source as str
    #[inline]
    pub fn slice_str(&self, start: u32, end: u32) -> &'a str {
        // Safety: OXC guarantees valid UTF-8 boundaries
        unsafe { std::str::from_utf8_unchecked(&self.source_bytes[start as usize..end as usize]) }
    }
}

/// Process an import declaration and return a ScriptImport item
pub fn process_import<'a>(
    import: &ImportDeclaration<'a>,
    ctx: &ScriptParseContext<'a>,
) -> ScriptImport<'a> {
    let mut bindings = Vec::new();

    // Extract bindings from specifiers
    if let Some(specifiers) = &import.specifiers {
        for spec in specifiers {
            match spec {
                ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    bindings.push(ScriptBinding {
                        name: s.local.name.as_str(),
                        span: ctx.adjust_span(s.local.span),
                    });
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    bindings.push(ScriptBinding {
                        name: s.local.name.as_str(),
                        span: ctx.adjust_span(s.local.span),
                    });
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    bindings.push(ScriptBinding {
                        name: s.local.name.as_str(),
                        span: ctx.adjust_span(s.local.span),
                    });
                }
            }
        }
    }

    ScriptImport {
        span: ctx.adjust_span(import.span),
        source: import.source.value.as_str(),
        source_span: ctx.adjust_span(import.source.span),
        bindings,
        is_type_only: import.import_kind.is_type(),
    }
}

/// Process a named export declaration and return a ScriptExport item
pub fn process_named_export<'a>(
    export: &ExportNamedDeclaration<'a>,
    ctx: &ScriptParseContext<'a>,
) -> ScriptExport<'a> {
    let mut bindings = Vec::new();

    // Extract bindings from export specifiers: export { a, b as c }
    for spec in &export.specifiers {
        // Use exported name (the name visible outside)
        let name = match &spec.exported {
            ModuleExportName::IdentifierName(id) => id.name.as_str(),
            ModuleExportName::IdentifierReference(id) => id.name.as_str(),
            ModuleExportName::StringLiteral(s) => s.value.as_str(),
        };
        bindings.push(ScriptBinding {
            name,
            span: ctx.adjust_span(spec.exported.span()),
        });
    }

    // Handle declaration exports: export const foo = 1
    if let Some(decl) = &export.declaration {
        extract_declaration_bindings(decl, ctx, &mut bindings);
    }

    ScriptExport {
        span: ctx.adjust_span(export.span),
        bindings,
        source: export.source.as_ref().map(|s| s.value.as_str()),
        is_type_only: export.export_kind.is_type(),
    }
}

/// Process an export all declaration: export * from 'foo'
pub fn process_all_export<'a>(
    export: &ExportAllDeclaration<'a>,
    ctx: &ScriptParseContext<'a>,
) -> ScriptExport<'a> {
    let bindings = if let Some(exported) = &export.exported {
        // export * as foo from 'bar'
        let name = match exported {
            ModuleExportName::IdentifierName(id) => id.name.as_str(),
            ModuleExportName::IdentifierReference(id) => id.name.as_str(),
            ModuleExportName::StringLiteral(s) => s.value.as_str(),
        };
        vec![ScriptBinding {
            name,
            span: ctx.adjust_span(exported.span()),
        }]
    } else {
        Vec::new()
    };

    ScriptExport {
        span: ctx.adjust_span(export.span),
        bindings,
        source: Some(export.source.value.as_str()),
        is_type_only: export.export_kind.is_type(),
    }
}

/// Extract bindings from a declaration (for export declarations)
fn extract_declaration_bindings<'a>(
    decl: &Declaration<'a>,
    ctx: &ScriptParseContext<'a>,
    bindings: &mut Vec<ScriptBinding<'a>>,
) {
    match decl {
        Declaration::VariableDeclaration(var_decl) => {
            for declarator in &var_decl.declarations {
                collect_binding_pattern_names(&declarator.id, ctx, bindings);
            }
        }
        Declaration::FunctionDeclaration(func) => {
            if let Some(id) = &func.id {
                bindings.push(ScriptBinding {
                    name: id.name.as_str(),
                    span: ctx.adjust_span(id.span),
                });
            }
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                bindings.push(ScriptBinding {
                    name: id.name.as_str(),
                    span: ctx.adjust_span(id.span),
                });
            }
        }
        // Type declarations don't produce runtime bindings
        Declaration::TSTypeAliasDeclaration(_)
        | Declaration::TSInterfaceDeclaration(_)
        | Declaration::TSEnumDeclaration(_)
        | Declaration::TSModuleDeclaration(_)
        | Declaration::TSImportEqualsDeclaration(_)
        | Declaration::TSGlobalDeclaration(_) => {}
    }
}

/// Collect binding names from a binding pattern (handles destructuring)
fn collect_binding_pattern_names<'a>(
    pattern: &BindingPattern<'a>,
    ctx: &ScriptParseContext<'a>,
    bindings: &mut Vec<ScriptBinding<'a>>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            bindings.push(ScriptBinding {
                name: id.name.as_str(),
                span: ctx.adjust_span(id.span),
            });
        }
        BindingPattern::ObjectPattern(obj) => {
            for prop in &obj.properties {
                collect_binding_pattern_names(&prop.value, ctx, bindings);
            }
            if let Some(rest) = &obj.rest {
                collect_binding_pattern_names(&rest.argument, ctx, bindings);
            }
        }
        BindingPattern::ArrayPattern(arr) => {
            for elem in arr.elements.iter().flatten() {
                collect_binding_pattern_names(elem, ctx, bindings);
            }
            if let Some(rest) = &arr.rest {
                collect_binding_pattern_names(&rest.argument, ctx, bindings);
            }
        }
        BindingPattern::AssignmentPattern(assign) => {
            collect_binding_pattern_names(&assign.left, ctx, bindings);
        }
    }
}

/// Try to process a statement as an import, returning Some if it is
pub fn try_process_import<'a>(
    stmt: &Statement<'a>,
    ctx: &ScriptParseContext<'a>,
) -> Option<ScriptItem<'a>> {
    match stmt {
        Statement::ImportDeclaration(import) => {
            Some(ScriptItem::Import(process_import(import, ctx)))
        }
        _ => None,
    }
}

/// Try to process a statement as an export (non-default), returning Some if it is
pub fn try_process_export<'a>(
    stmt: &Statement<'a>,
    ctx: &ScriptParseContext<'a>,
) -> Option<ScriptItem<'a>> {
    match stmt {
        Statement::ExportNamedDeclaration(export) => {
            Some(ScriptItem::Export(process_named_export(export, ctx)))
        }
        Statement::ExportAllDeclaration(export) => {
            Some(ScriptItem::Export(process_all_export(export, ctx)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjust_span() {
        let ctx = ScriptParseContext::new(100, b"const x = 1;");
        let oxc_span = oxc_span::Span::new(0, 12);
        let adjusted = ctx.adjust_span(oxc_span);
        assert_eq!(adjusted.start, 100);
        assert_eq!(adjusted.end, 112);
    }
}
