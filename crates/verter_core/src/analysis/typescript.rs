//! TypeScript/JavaScript file parsing for cross-file analysis.
//!
//! This module provides parsing for standalone TypeScript and JavaScript files
//! (not Vue SFCs) to enable cross-file analysis of composables, utilities,
//! and shared modules that use Vue APIs.
//!
//! # Usage
//!
//! ```ignore
//! use verter_core::analysis::typescript::{parse_typescript_file, TypeScriptFileInfo};
//!
//! let source = r#"
//! import { provide, ref } from 'vue';
//!
//! export function useTheme() {
//!     const theme = ref('light');
//!     provide('theme', theme);
//!     return theme;
//! }
//! "#;
//!
//! let info = parse_typescript_file(source, true)?;
//! println!("Provides: {:?}", info.provides);
//! println!("Exports: {:?}", info.exports);
//! ```

#![allow(dead_code)]

use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};

use crate::common::Span;
use crate::utils::oxc::vue::{
    detect_vue_api_call, EmitCallUsage, InjectUsage, LifecycleHook, LifecycleUsage, ProvideKey,
    ProvideKeyKind, ProvideUsage, ReactiveKind, ReactiveStateUsage, UsageCollector, VueApiKind,
    WatcherUsage,
};

// Re-import FileUsageFlags from file_usage for HAS_IMPORTS/HAS_EXPORTS
use super::file_usage::FileUsageFlags;

// =============================================================================
// TypeScript File Info
// =============================================================================

/// Information extracted from a TypeScript/JavaScript file.
#[derive(Debug, Default)]
pub struct TypeScriptFileInfo {
    /// Import declarations
    pub imports: Vec<TypeScriptImport>,
    /// Export declarations (named and default)
    pub exports: Vec<TypeScriptExport>,
    /// Provide calls found in the file
    pub provides: Vec<ProvideUsage>,
    /// Inject calls found in the file
    pub injects: Vec<InjectUsage>,
    /// Lifecycle hook calls
    pub lifecycle: Vec<LifecycleUsage>,
    /// Reactive state definitions
    pub reactive: Vec<ReactiveStateUsage>,
    /// Watcher definitions
    pub watchers: Vec<WatcherUsage>,
    /// Emit calls (in composables that receive emit as parameter)
    pub emit_calls: Vec<EmitCallUsage>,
    /// Quick lookup flags
    pub flags: FileUsageFlags,
    /// Whether the file contains async functions with await
    pub has_async: bool,
    /// Parse errors (if any)
    pub errors: Vec<ParseError>,
}

/// An import declaration
#[derive(Debug, Clone)]
pub struct TypeScriptImport {
    /// Span of the entire import statement
    pub span: Span,
    /// The module specifier
    pub source: String,
    /// Imported bindings
    pub bindings: Vec<ImportBinding>,
    /// Whether this is a type-only import
    pub is_type_only: bool,
}

/// An imported binding
#[derive(Debug, Clone)]
pub struct ImportBinding {
    /// Local name of the binding
    pub local: String,
    /// Imported name (different if aliased)
    pub imported: Option<String>,
    /// Span of the local name
    pub span: Span,
}

/// An export declaration
#[derive(Debug, Clone)]
pub struct TypeScriptExport {
    /// Span of the export
    pub span: Span,
    /// Kind of export
    pub kind: ExportKind,
    /// Whether this is a type-only export
    pub is_type_only: bool,
}

/// Kind of export
#[derive(Debug, Clone)]
pub enum ExportKind {
    /// Named export: export { foo, bar }
    Named { names: Vec<ExportedName> },
    /// Default export: export default ...
    Default { name: Option<String> },
    /// Re-export: export * from 'module'
    All { source: String },
    /// Function export: export function foo() {}
    Function { name: String, is_async: bool },
    /// Variable export: export const foo = ...
    Variable { name: String },
    /// Class export: export class Foo {}
    Class { name: String },
}

/// An exported name
#[derive(Debug, Clone)]
pub struct ExportedName {
    /// Local name
    pub local: String,
    /// Exported name (different if aliased)
    pub exported: Option<String>,
    /// Span
    pub span: Span,
}

/// A parse error
#[derive(Debug, Clone)]
pub struct ParseError {
    /// Span where error occurred
    pub span: Span,
    /// Error message
    pub message: String,
}

// =============================================================================
// Parsing
// =============================================================================

/// Parse a TypeScript or JavaScript file.
///
/// # Arguments
///
/// * `source` - The source code to parse
/// * `is_typescript` - Whether to parse as TypeScript (true) or JavaScript (false)
///
/// # Returns
///
/// A `TypeScriptFileInfo` containing extracted information.
pub fn parse_typescript_file(source: &str, is_typescript: bool) -> TypeScriptFileInfo {
    let allocator = Allocator::default();
    let source_type = if is_typescript {
        SourceType::tsx()
    } else {
        SourceType::mjs()
    };

    let parser_result = Parser::new(&allocator, source, source_type).parse();

    let mut info = TypeScriptFileInfo::default();

    // Collect parse errors
    for error in parser_result.errors {
        info.errors.push(ParseError {
            span: Span::new(0, 0), // OXC errors don't always have spans
            message: error.to_string(),
        });
    }

    // Process the AST
    let mut collector = UsageCollector::new(source.as_bytes());

    for stmt in &parser_result.program.body {
        process_statement(stmt, source, &mut info, &mut collector);
    }

    // Transfer collected usage
    info.provides = collector.provides;
    info.injects = collector.injects;
    info.lifecycle = collector.lifecycle;
    info.reactive = collector.reactive;
    info.watchers = collector.watchers;
    info.emit_calls = collector.emit_calls;

    // Merge collector flags into info flags (they have compatible bit layouts)
    // The usage::FileUsageFlags and file_usage::FileUsageFlags share the same bit positions
    // for Vue API flags (HAS_PROVIDE, HAS_INJECT, etc.)
    let collector_bits = collector.flags.bits();
    info.flags = FileUsageFlags::from_bits(info.flags.bits() | collector_bits);

    info
}

/// Parse with a specific file extension to determine source type.
pub fn parse_file_by_extension(source: &str, extension: &str) -> TypeScriptFileInfo {
    let is_typescript = matches!(
        extension.to_lowercase().as_str(),
        "ts" | "tsx" | "mts" | "cts"
    );
    parse_typescript_file(source, is_typescript)
}

// =============================================================================
// Statement Processing
// =============================================================================

fn process_statement(
    stmt: &Statement<'_>,
    source: &str,
    info: &mut TypeScriptFileInfo,
    collector: &mut UsageCollector<'_>,
) {
    match stmt {
        Statement::ImportDeclaration(import) => {
            process_import(import, source, info);
        }
        Statement::ExportNamedDeclaration(export) => {
            process_named_export(export, source, info, collector);
        }
        Statement::ExportDefaultDeclaration(export) => {
            process_default_export(export, source, info, collector);
        }
        Statement::ExportAllDeclaration(export) => {
            let source_str = export.source.value.to_string();
            info.exports.push(TypeScriptExport {
                span: Span::new(export.span.start, export.span.end),
                kind: ExportKind::All { source: source_str },
                is_type_only: export.export_kind.is_type(),
            });
        }
        Statement::FunctionDeclaration(func) => {
            // Check function body for Vue API usage
            if let Some(body) = &func.body {
                process_function_body(body, collector);
            }
            if func.r#async {
                info.has_async = true;
            }
        }
        Statement::VariableDeclaration(var_decl) => {
            process_variable_declaration(var_decl, collector);
        }
        Statement::ExpressionStatement(expr_stmt) => {
            process_expression(&expr_stmt.expression, collector, None);
        }
        _ => {}
    }
}

fn process_import(import: &ImportDeclaration<'_>, _source: &str, info: &mut TypeScriptFileInfo) {
    let mut bindings = Vec::new();

    if let Some(specifiers) = &import.specifiers {
        for spec in specifiers {
            match spec {
                oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    bindings.push(ImportBinding {
                        local: s.local.name.to_string(),
                        imported: if s.imported.name() != s.local.name.as_str() {
                            Some(s.imported.name().to_string())
                        } else {
                            None
                        },
                        span: Span::new(s.local.span.start, s.local.span.end),
                    });
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    bindings.push(ImportBinding {
                        local: s.local.name.to_string(),
                        imported: Some("default".to_string()),
                        span: Span::new(s.local.span.start, s.local.span.end),
                    });
                }
                oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    bindings.push(ImportBinding {
                        local: s.local.name.to_string(),
                        imported: Some("*".to_string()),
                        span: Span::new(s.local.span.start, s.local.span.end),
                    });
                }
            }
        }
    }

    info.imports.push(TypeScriptImport {
        span: Span::new(import.span.start, import.span.end),
        source: import.source.value.to_string(),
        bindings,
        is_type_only: import.import_kind.is_type(),
    });

    info.flags.set(FileUsageFlags::HAS_IMPORTS);
}

fn process_named_export(
    export: &ExportNamedDeclaration<'_>,
    _source: &str,
    info: &mut TypeScriptFileInfo,
    collector: &mut UsageCollector<'_>,
) {
    info.flags.set(FileUsageFlags::HAS_EXPORTS);

    // Handle re-exports: export { foo } from 'module'
    if export.source.is_some() {
        let names: Vec<ExportedName> = export
            .specifiers
            .iter()
            .map(|s| ExportedName {
                local: s.local.name().to_string(),
                exported: if s.exported.name() != s.local.name() {
                    Some(s.exported.name().to_string())
                } else {
                    None
                },
                span: Span::new(s.span.start, s.span.end),
            })
            .collect();

        info.exports.push(TypeScriptExport {
            span: Span::new(export.span.start, export.span.end),
            kind: ExportKind::Named { names },
            is_type_only: export.export_kind.is_type(),
        });
        return;
    }

    // Handle export declarations: export function foo() {}
    if let Some(decl) = &export.declaration {
        match decl {
            Declaration::FunctionDeclaration(func) => {
                let name = func
                    .id
                    .as_ref()
                    .map(|id| id.name.to_string())
                    .unwrap_or_default();

                info.exports.push(TypeScriptExport {
                    span: Span::new(export.span.start, export.span.end),
                    kind: ExportKind::Function {
                        name,
                        is_async: func.r#async,
                    },
                    is_type_only: false,
                });

                // Process function body for Vue API usage
                if let Some(body) = &func.body {
                    process_function_body(body, collector);
                }

                if func.r#async {
                    info.has_async = true;
                }
            }
            Declaration::VariableDeclaration(var_decl) => {
                for declarator in &var_decl.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        info.exports.push(TypeScriptExport {
                            span: Span::new(export.span.start, export.span.end),
                            kind: ExportKind::Variable {
                                name: id.name.to_string(),
                            },
                            is_type_only: false,
                        });
                    }

                    // Check initializer for Vue API usage
                    if let Some(init) = &declarator.init {
                        let binding_span = match &declarator.id {
                            BindingPattern::BindingIdentifier(id) => {
                                Some(Span::new(id.span.start, id.span.end))
                            }
                            _ => None,
                        };
                        process_expression(init, collector, binding_span);
                    }
                }
            }
            Declaration::ClassDeclaration(class) => {
                let name = class
                    .id
                    .as_ref()
                    .map(|id| id.name.to_string())
                    .unwrap_or_default();

                info.exports.push(TypeScriptExport {
                    span: Span::new(export.span.start, export.span.end),
                    kind: ExportKind::Class { name },
                    is_type_only: false,
                });
            }
            _ => {}
        }
        return;
    }

    // Handle export specifiers: export { foo, bar }
    if !export.specifiers.is_empty() {
        let names: Vec<ExportedName> = export
            .specifiers
            .iter()
            .map(|s| ExportedName {
                local: s.local.name().to_string(),
                exported: if s.exported.name() != s.local.name() {
                    Some(s.exported.name().to_string())
                } else {
                    None
                },
                span: Span::new(s.span.start, s.span.end),
            })
            .collect();

        info.exports.push(TypeScriptExport {
            span: Span::new(export.span.start, export.span.end),
            kind: ExportKind::Named { names },
            is_type_only: export.export_kind.is_type(),
        });
    }
}

fn process_default_export(
    export: &ExportDefaultDeclaration<'_>,
    _source: &str,
    info: &mut TypeScriptFileInfo,
    collector: &mut UsageCollector<'_>,
) {
    info.flags.set(FileUsageFlags::HAS_EXPORTS);

    match &export.declaration {
        ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
            let name = func.id.as_ref().map(|id| id.name.to_string());

            info.exports.push(TypeScriptExport {
                span: Span::new(export.span.start, export.span.end),
                kind: ExportKind::Default { name },
                is_type_only: false,
            });

            if let Some(body) = &func.body {
                process_function_body(body, collector);
            }

            if func.r#async {
                info.has_async = true;
            }
        }
        ExportDefaultDeclarationKind::ClassDeclaration(class) => {
            let name = class.id.as_ref().map(|id| id.name.to_string());

            info.exports.push(TypeScriptExport {
                span: Span::new(export.span.start, export.span.end),
                kind: ExportKind::Default { name },
                is_type_only: false,
            });
        }
        _ => {
            info.exports.push(TypeScriptExport {
                span: Span::new(export.span.start, export.span.end),
                kind: ExportKind::Default { name: None },
                is_type_only: false,
            });
        }
    }
}

fn process_variable_declaration(
    var_decl: &oxc_ast::ast::VariableDeclaration<'_>,
    collector: &mut UsageCollector<'_>,
) {
    for declarator in &var_decl.declarations {
        if let Some(init) = &declarator.init {
            let binding_span = match &declarator.id {
                BindingPattern::BindingIdentifier(id) => {
                    Some(Span::new(id.span.start, id.span.end))
                }
                _ => None,
            };
            process_expression(init, collector, binding_span);
        }
    }
}

fn process_function_body(body: &FunctionBody<'_>, collector: &mut UsageCollector<'_>) {
    for stmt in &body.statements {
        process_body_statement(stmt, collector);
    }
}

fn process_body_statement(stmt: &Statement<'_>, collector: &mut UsageCollector<'_>) {
    match stmt {
        Statement::VariableDeclaration(var_decl) => {
            process_variable_declaration(var_decl, collector);
        }
        Statement::ExpressionStatement(expr_stmt) => {
            process_expression(&expr_stmt.expression, collector, None);
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                process_expression(arg, collector, None);
            }
        }
        Statement::IfStatement(if_stmt) => {
            process_expression(&if_stmt.test, collector, None);
            process_body_statement(&if_stmt.consequent, collector);
            if let Some(alt) = &if_stmt.alternate {
                process_body_statement(alt, collector);
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                process_body_statement(s, collector);
            }
        }
        Statement::FunctionDeclaration(func) => {
            if let Some(body) = &func.body {
                process_function_body(body, collector);
            }
        }
        _ => {}
    }
}

fn process_expression(
    expr: &Expression<'_>,
    collector: &mut UsageCollector<'_>,
    binding_span: Option<Span>,
) {
    match expr {
        Expression::CallExpression(call) => {
            process_call_expression(call, collector, binding_span);

            // Process arguments recursively
            for arg in &call.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    process_expression(&spread.argument, collector, None);
                } else if let Some(expr) = arg.as_expression() {
                    process_expression(expr, collector, None);
                }
            }
        }
        Expression::ArrowFunctionExpression(arrow) => {
            // Process arrow function body
            if arrow.expression {
                // Single expression body
                for stmt in &arrow.body.statements {
                    if let Statement::ExpressionStatement(expr_stmt) = stmt {
                        process_expression(&expr_stmt.expression, collector, None);
                    }
                }
            } else {
                process_function_body(&arrow.body, collector);
            }
        }
        Expression::FunctionExpression(func) => {
            if let Some(body) = &func.body {
                process_function_body(body, collector);
            }
        }
        Expression::ConditionalExpression(cond) => {
            process_expression(&cond.test, collector, None);
            process_expression(&cond.consequent, collector, None);
            process_expression(&cond.alternate, collector, None);
        }
        Expression::LogicalExpression(logical) => {
            process_expression(&logical.left, collector, None);
            process_expression(&logical.right, collector, None);
        }
        Expression::BinaryExpression(binary) => {
            process_expression(&binary.left, collector, None);
            process_expression(&binary.right, collector, None);
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop {
                    process_expression(&p.value, collector, None);
                }
            }
        }
        Expression::ArrayExpression(arr) => {
            for elem in &arr.elements {
                if let oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) = elem {
                    process_expression(&spread.argument, collector, None);
                } else if let Some(expr) = elem.as_expression() {
                    process_expression(expr, collector, None);
                }
            }
        }
        _ => {}
    }
}

fn process_call_expression(
    call: &CallExpression<'_>,
    collector: &mut UsageCollector<'_>,
    binding_span: Option<Span>,
) {
    // Check if this is a Vue API call
    if let Expression::Identifier(id) = &call.callee {
        let name = id.name.as_bytes();

        if let Some(api_kind) = detect_vue_api_call(name) {
            collect_api_usage(call, api_kind, collector, binding_span);
        }
    }
}

fn collect_api_usage(
    call: &CallExpression<'_>,
    api_kind: VueApiKind,
    collector: &mut UsageCollector<'_>,
    binding_span: Option<Span>,
) {
    let call_span = Span::new(call.span.start, call.span.end);

    match api_kind {
        VueApiKind::Provide => {
            if let Some(key) = extract_provide_key(call) {
                let value_span = call
                    .arguments
                    .get(1)
                    .and_then(|a| a.as_expression())
                    .map(|e| Span::new(e.span().start, e.span().end))
                    .unwrap_or(call_span);

                collector.record_provide(ProvideUsage {
                    span: call_span,
                    key,
                    value_span,
                });
            }
        }
        VueApiKind::Inject => {
            if let Some(key) = extract_provide_key(call) {
                let has_default = call.arguments.len() > 1;

                collector.record_inject(InjectUsage {
                    span: call_span,
                    key,
                    has_default,
                    binding_span,
                });
            }
        }
        VueApiKind::Ref
        | VueApiKind::ShallowRef
        | VueApiKind::Reactive
        | VueApiKind::ShallowReactive
        | VueApiKind::Computed => {
            if let Some(binding) = binding_span {
                let reactive_kind = match api_kind {
                    VueApiKind::Ref => ReactiveKind::Ref,
                    VueApiKind::ShallowRef => ReactiveKind::ShallowRef,
                    VueApiKind::Reactive => ReactiveKind::Reactive,
                    VueApiKind::ShallowReactive => ReactiveKind::ShallowReactive,
                    VueApiKind::Computed => ReactiveKind::Computed,
                    _ => unreachable!(),
                };

                let initializer_span = call
                    .arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .map(|e| Span::new(e.span().start, e.span().end));

                collector.record_reactive(ReactiveStateUsage {
                    kind: reactive_kind,
                    binding_span: binding,
                    initializer_span,
                });
            }
        }
        VueApiKind::OnMounted
        | VueApiKind::OnUnmounted
        | VueApiKind::OnBeforeMount
        | VueApiKind::OnBeforeUnmount
        | VueApiKind::OnUpdated
        | VueApiKind::OnBeforeUpdate
        | VueApiKind::OnErrorCaptured
        | VueApiKind::OnActivated
        | VueApiKind::OnDeactivated
        | VueApiKind::OnRenderTracked
        | VueApiKind::OnRenderTriggered
        | VueApiKind::OnServerPrefetch => {
            if let Some(hook) = LifecycleHook::from_api_kind(api_kind) {
                let callback_span = call
                    .arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .map(|e| Span::new(e.span().start, e.span().end))
                    .unwrap_or(call_span);

                collector.record_lifecycle(LifecycleUsage {
                    span: call_span,
                    hook,
                    callback_span,
                });
            }
        }
        VueApiKind::Watch
        | VueApiKind::WatchEffect
        | VueApiKind::WatchPostEffect
        | VueApiKind::WatchSyncEffect => {
            let callback_span = if api_kind == VueApiKind::Watch {
                // watch(source, callback)
                call.arguments
                    .get(1)
                    .and_then(|a| a.as_expression())
                    .map(|e| Span::new(e.span().start, e.span().end))
                    .unwrap_or(call_span)
            } else {
                // watchEffect(callback)
                call.arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .map(|e| Span::new(e.span().start, e.span().end))
                    .unwrap_or(call_span)
            };

            let source_spans = if api_kind == VueApiKind::Watch {
                call.arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .map(|e| vec![Span::new(e.span().start, e.span().end)])
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            collector.record_watcher(WatcherUsage {
                span: call_span,
                kind: api_kind,
                callback_span,
                source_spans,
            });
        }
        _ => {}
    }
}

fn extract_provide_key(call: &CallExpression<'_>) -> Option<ProvideKey> {
    let first_arg = call.arguments.first()?.as_expression()?;

    match first_arg {
        Expression::StringLiteral(s) => Some(ProvideKey {
            span: Span::new(s.span.start, s.span.end),
            kind: ProvideKeyKind::StringLiteral,
        }),
        Expression::Identifier(id) => Some(ProvideKey {
            span: Span::new(id.span.start, id.span.end),
            kind: ProvideKeyKind::Symbol,
        }),
        _ => Some(ProvideKey {
            span: Span::new(first_arg.span().start, first_arg.span().end),
            kind: ProvideKeyKind::Dynamic,
        }),
    }
}

// =============================================================================
// Conversion to FileUsageInfoOwned
// =============================================================================

impl TypeScriptFileInfo {
    /// Convert to FileUsageInfoOwned for use with ProjectIndex.
    pub fn to_file_usage_owned(&self, source: &[u8]) -> super::file_usage::FileUsageInfoOwned {
        use super::file_usage::{ImportInfoOwned, InjectUsageOwned, ProvideUsageOwned};
        use crate::utils::oxc::vue::ProvideKeyKind;

        let extract = |span: Span| -> Option<String> {
            let start = span.start as usize;
            let end = span.end as usize;
            if end <= source.len() {
                std::str::from_utf8(&source[start..end])
                    .ok()
                    .map(|s| s.to_string())
            } else {
                None
            }
        };

        let imports = self
            .imports
            .iter()
            .map(|imp| ImportInfoOwned {
                source: imp.source.clone(),
                bindings: imp.bindings.iter().map(|b| b.local.clone()).collect(),
                is_type_only: imp.is_type_only,
                start: imp.span.start,
                end: imp.span.end,
            })
            .collect();

        let provides = self
            .provides
            .iter()
            .map(|p| {
                let (key, is_dynamic) = match p.key.kind {
                    ProvideKeyKind::StringLiteral => (extract(p.key.span), false),
                    ProvideKeyKind::Symbol => (extract(p.key.span), false),
                    ProvideKeyKind::Dynamic => (None, true),
                };
                ProvideUsageOwned {
                    key,
                    is_dynamic_key: is_dynamic,
                    start: p.span.start,
                    end: p.span.end,
                }
            })
            .collect();

        let injects = self
            .injects
            .iter()
            .map(|i| {
                let (key, is_dynamic) = match i.key.kind {
                    ProvideKeyKind::StringLiteral => (extract(i.key.span), false),
                    ProvideKeyKind::Symbol => (extract(i.key.span), false),
                    ProvideKeyKind::Dynamic => (None, true),
                };
                InjectUsageOwned {
                    key,
                    is_dynamic_key: is_dynamic,
                    has_default: i.has_default,
                    binding_name: i.binding_span.and_then(&extract),
                    start: i.span.start,
                    end: i.span.end,
                }
            })
            .collect();

        super::file_usage::FileUsageInfoOwned {
            imports,
            macros: Vec::new(), // TS files don't have Vue macros
            provides,
            injects,
            components: Vec::new(), // TS files don't have template components
            flags: self.flags.bits(),
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_composable() {
        let source = r#"
import { ref, provide } from 'vue';

export function useCounter() {
    const count = ref(0);
    provide('counter', count);
    return count;
}
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "vue");
        assert_eq!(info.imports[0].bindings.len(), 2);

        assert_eq!(info.exports.len(), 1);
        assert!(
            matches!(&info.exports[0].kind, ExportKind::Function { name, .. } if name == "useCounter")
        );

        assert_eq!(info.provides.len(), 1);
        assert_eq!(info.reactive.len(), 1);
    }

    #[test]
    fn test_parse_composable_with_inject() {
        let source = r#"
import { inject, computed } from 'vue';

export function useTheme() {
    const theme = inject('theme', 'light');
    const isDark = computed(() => theme === 'dark');
    return { theme, isDark };
}
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.injects.len(), 1);
        assert!(info.injects[0].has_default);

        assert_eq!(info.reactive.len(), 1);
        assert_eq!(info.reactive[0].kind, ReactiveKind::Computed);
    }

    #[test]
    fn test_parse_composable_with_lifecycle() {
        let source = r#"
import { ref, onMounted, onUnmounted } from 'vue';

export function useWindowSize() {
    const width = ref(0);
    const height = ref(0);

    onMounted(() => {
        width.value = window.innerWidth;
        height.value = window.innerHeight;
    });

    onUnmounted(() => {
        // cleanup
    });

    return { width, height };
}
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.reactive.len(), 2);
        assert_eq!(info.lifecycle.len(), 2);

        let hooks: Vec<_> = info.lifecycle.iter().map(|l| l.hook).collect();
        assert!(hooks.contains(&LifecycleHook::OnMounted));
        assert!(hooks.contains(&LifecycleHook::OnUnmounted));
    }

    #[test]
    fn test_parse_composable_with_watchers() {
        let source = r#"
import { ref, watch, watchEffect } from 'vue';

export function useSearch(query) {
    const results = ref([]);

    watch(query, (newQuery) => {
        // search logic
    });

    watchEffect(() => {
        console.log(results.value);
    });

    return results;
}
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.watchers.len(), 2);
        assert!(info.watchers.iter().any(|w| w.kind == VueApiKind::Watch));
        assert!(info
            .watchers
            .iter()
            .any(|w| w.kind == VueApiKind::WatchEffect));
    }

    #[test]
    fn test_parse_named_exports() {
        let source = r#"
export const API_URL = 'https://api.example.com';
export function fetchData() {}
export class ApiClient {}
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.exports.len(), 3);
        assert!(
            matches!(&info.exports[0].kind, ExportKind::Variable { name } if name == "API_URL")
        );
        assert!(
            matches!(&info.exports[1].kind, ExportKind::Function { name, .. } if name == "fetchData")
        );
        assert!(matches!(&info.exports[2].kind, ExportKind::Class { name } if name == "ApiClient"));
    }

    #[test]
    fn test_parse_default_export() {
        let source = r#"
export default function useFeature() {
    return {};
}
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.exports.len(), 1);
        assert!(
            matches!(&info.exports[0].kind, ExportKind::Default { name } if name.as_deref() == Some("useFeature"))
        );
    }

    #[test]
    fn test_parse_re_exports() {
        let source = r#"
export * from './utils';
export { foo, bar as baz } from './other';
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.exports.len(), 2);
        assert!(matches!(&info.exports[0].kind, ExportKind::All { source } if source == "./utils"));
        assert!(matches!(&info.exports[1].kind, ExportKind::Named { names } if names.len() == 2));
    }

    #[test]
    fn test_parse_type_only_imports() {
        let source = r#"
import type { Ref } from 'vue';
import { ref } from 'vue';
"#;

        let info = parse_typescript_file(source, true);

        assert_eq!(info.imports.len(), 2);
        assert!(info.imports[0].is_type_only);
        assert!(!info.imports[1].is_type_only);
    }

    #[test]
    fn test_parse_javascript_file() {
        let source = r#"
import { ref } from 'vue';

export function useCounter() {
    const count = ref(0);
    return count;
}
"#;

        let info = parse_typescript_file(source, false);

        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.exports.len(), 1);
        assert_eq!(info.reactive.len(), 1);
    }

    #[test]
    fn test_to_file_usage_owned() {
        let source = r#"
import { provide, inject } from 'vue';

export function useConfig() {
    provide('config', { debug: true });
}

export function useConfigConsumer() {
    const config = inject('config');
    return config;
}
"#;

        let info = parse_typescript_file(source, true);
        let owned = info.to_file_usage_owned(source.as_bytes());

        assert_eq!(owned.imports.len(), 1);
        assert_eq!(owned.provides.len(), 1);
        assert_eq!(owned.injects.len(), 1);

        // Check that keys were extracted
        assert_eq!(owned.provides[0].key.as_deref(), Some("'config'"));
        assert_eq!(owned.injects[0].key.as_deref(), Some("'config'"));
    }

    #[test]
    fn test_flags_are_set() {
        let source = r#"
import { provide, inject, ref, onMounted, watch } from 'vue';

export function useFeature() {
    const value = ref(0);
    provide('key', value);
    inject('other');
    onMounted(() => {});
    watch(value, () => {});
}
"#;

        let info = parse_typescript_file(source, true);

        assert!(info.flags.has(FileUsageFlags::HAS_IMPORTS));
        assert!(info.flags.has(FileUsageFlags::HAS_EXPORTS));
        assert!(info.flags.has(FileUsageFlags::HAS_PROVIDE));
        assert!(info.flags.has(FileUsageFlags::HAS_INJECT));
        assert!(info.flags.has(FileUsageFlags::HAS_REACTIVE_STATE));
        assert!(info.flags.has(FileUsageFlags::HAS_LIFECYCLE_HOOKS));
        assert!(info.flags.has(FileUsageFlags::HAS_WATCHERS));
    }
}
