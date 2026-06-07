//! External (cross-file) type resolution for Vue compiler macros.
//!
//! Resolves a type symbol referenced in one file from another file's source by
//! parsing the dependency file, building a type resolution context against it,
//! and applying the same shared resolution kernel that local types use. This
//! module owns:
//!
//! - The `AnalyzedExternalTypeSource` shallow inventory built from a parsed
//!   dependency program (imports, re-exports, exported local type symbols, and
//!   their dependency edges).
//! - The cold-path entry points (`resolve_external_type*`) used by hosts that
//!   pre-resolve dependency types.
//! - The structural-dependency walkers used by the inventory to record what an
//!   exported type structurally references inside the same file (so the host
//!   knows which other dependency names must be available when resolving the
//!   exported type).
//!
//! The same five-mode resolution kernel applies (Identity, Navigate, Shallow,
//! Expanded, Skeleton) — see `/type-resolution`. Cross-file traversal walks
//! only the import graph reachable from the requested type's declaration
//! graph; barrel hops are handled by the host on the other side of these
//! entry points.
//!
//! `hash_resolved_type` lives in this module because every external resolution
//! entry point is a natural consumer.

use std::str;

use oxc_ast::ast::*;
use rustc_hash::{FxHashMap, FxHashSet};

use verter_type_expr::TypeExprScope;

use crate::common::Span;

use super::{
    build_type_context, extract_heritage_type_names, get_expression_reference_name,
    get_type_reference_name, resolve_class_with_heritage_ctx_ref,
    resolve_interface_with_extends_ctx_ref, resolve_named_local_type_with_ctx_ref,
    resolve_value_declaration_type, ResolvedElements, RuntimeType, TypeResolutionContext,
};
use crate::utils::oxc::vue::script::raw_surface::{
    capture_statement_surfaces, merge_overload_groups, RawSourceSurface, SymbolSpace,
};

/// Resolve an imported type by name from a dependency file's source.
///
/// Parses the dep file, builds a type resolution context, finds the named type
/// (interface or type alias), and resolves it to structured property/emit information.
///
/// Returns `None` if the file can't be parsed or the named type isn't found.
///
/// `external_canonical_id` is the canonical_id of the dependency file whose
/// source is being resolved; it is stamped onto every populated `type_expr`
/// as the paired `type_expr_scope`. Test callers without a canonical_id may
/// pass an empty string — the pairing invariant is satisfied either way.
pub fn resolve_external_type_with_canonical(
    type_name: &str,
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
    external_canonical_id: &str,
) -> Option<ResolvedElements> {
    resolve_external_type_with_companion_and_canonical(
        type_name,
        dep_source,
        &FxHashMap::default(),
        allocator,
        external_canonical_id,
    )
}

/// Backward-compatible wrapper that resolves without a canonical_id. The
/// resulting `type_expr_scope` carries an empty string; production callers
/// should prefer `resolve_external_type_with_canonical` so the paired scope
/// is meaningful.
pub fn resolve_external_type(
    type_name: &str,
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> Option<ResolvedElements> {
    resolve_external_type_with_canonical(type_name, dep_source, allocator, "")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTypeBinding {
    pub local_name: String,
    pub imported_name: String,
    pub source: String,
    pub is_namespace: bool,
}

/// Result of extracting type bindings from a dependency file.
/// Includes named bindings (from `import` and `export {} from`) and
/// wildcard re-export sources (from `export * from`).
#[derive(Debug, Clone, Default)]
pub struct ExtractedTypeBindings {
    pub bindings: Vec<ImportedTypeBinding>,
    pub reexport_bindings: Vec<ImportedTypeBinding>,
    pub wildcard_reexport_sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyzedExternalTypeSymbolKind {
    TypeAlias,
    Interface,
    Class,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedExternalTypeSymbol {
    pub kind: AnalyzedExternalTypeSymbolKind,
    pub span: Span,
    pub dependency_names: FxHashSet<String>,
    pub structural_dependency_names: FxHashSet<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AnalyzedExternalTypeSource {
    pub extracted: ExtractedTypeBindings,
    import_locals: FxHashSet<String>,
    direct_reexport_targets: FxHashMap<String, (String, String)>,
    local_import_symbol_targets: FxHashMap<String, (String, String)>,
    local_export_symbol_targets: FxHashMap<String, String>,
    exported_local_type_names: FxHashSet<String>,
    local_type_symbols: FxHashMap<String, AnalyzedExternalTypeSymbol>,
    top_level_statement_count: usize,
    /// Parse-time `RawSourceSurface` raw-fact inventory (oracle harness design
    /// item G), keyed by `(name, symbol_space)` within this file. Captured while
    /// the OXC arena is live, before lowering erases the §Q2 facts.
    raw_source_surfaces: FxHashMap<(String, SymbolSpace), Vec<RawSourceSurface>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnalyzedExternalTypeSourceStats {
    pub top_level_statement_count: usize,
    pub binding_count: usize,
    pub direct_reexport_count: usize,
    pub wildcard_reexport_count: usize,
    pub import_local_count: usize,
    pub local_type_symbol_count: usize,
    pub local_export_symbol_count: usize,
}

impl AnalyzedExternalTypeSource {
    pub fn required_import_names(&self, type_name: &str) -> FxHashSet<String> {
        let mut required_imports = FxHashSet::default();
        let mut visited = FxHashSet::default();
        let mut pending = vec![type_name.to_string()];

        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }

            if self.import_locals.contains(&current) {
                required_imports.insert(current);
                continue;
            }

            if let Some(symbol) = self.local_type_symbols.get(&current) {
                enqueue_required_import_refs(
                    symbol.structural_dependency_names.clone(),
                    &self.import_locals,
                    &mut required_imports,
                    &mut pending,
                    &visited,
                );
            }
        }

        required_imports
    }

    pub fn direct_reexport_target(&self, exported_name: &str) -> Option<(&str, &str)> {
        self.direct_reexport_targets
            .get(exported_name)
            .map(|(source, imported_name)| (source.as_str(), imported_name.as_str()))
    }

    pub fn local_import_symbol_target(&self, local_name: &str) -> Option<(&str, &str)> {
        self.local_import_symbol_targets
            .get(local_name)
            .map(|(source, imported_name)| (source.as_str(), imported_name.as_str()))
    }

    pub fn local_export_symbol_target(&self, exported_name: &str) -> Option<&str> {
        self.local_export_symbol_targets
            .get(exported_name)
            .map(|name| name.as_str())
    }

    pub fn exported_local_type_names(&self) -> impl Iterator<Item = &str> {
        self.exported_local_type_names.iter().map(String::as_str)
    }

    pub fn exported_local_symbol_names(&self) -> impl Iterator<Item = &str> {
        self.local_export_symbol_targets.keys().map(String::as_str)
    }

    pub fn direct_reexport_entries(&self) -> impl Iterator<Item = (&str, &str, &str)> {
        self.direct_reexport_targets
            .iter()
            .map(|(exported_name, (source, imported_name))| {
                (
                    exported_name.as_str(),
                    source.as_str(),
                    imported_name.as_str(),
                )
            })
    }

    pub fn wildcard_reexport_sources(&self) -> &[String] {
        &self.extracted.wildcard_reexport_sources
    }

    pub fn local_symbol_span(&self, symbol_name: &str) -> Option<Span> {
        self.local_type_symbols
            .get(symbol_name)
            .map(|symbol| symbol.span)
    }

    /// Enumerate every locally-declared type symbol name (interface /
    /// type-alias / class) paired with its full declaration span.
    ///
    /// Used by the imported-macro-surface JSDoc reattachment to scope a
    /// source-text JSDoc search to the SINGLE declaration that declares a
    /// given member, rather than a file-wide first match — when a file
    /// declares the same property name in two declarations, scoping to the
    /// declaring declaration's span attaches the correct leading JSDoc.
    pub fn local_type_symbol_spans(&self) -> impl Iterator<Item = (&str, Span)> {
        self.local_type_symbols
            .iter()
            .map(|(name, symbol)| (name.as_str(), symbol.span))
    }

    pub fn local_type_symbol(&self, symbol_name: &str) -> Option<&AnalyzedExternalTypeSymbol> {
        self.local_type_symbols.get(symbol_name)
    }

    pub fn local_symbol_target_name(&self, requested_name: &str) -> String {
        let mut current = requested_name.to_string();
        let mut visited = FxHashSet::default();

        while visited.insert(current.clone()) {
            let Some(next) = self.local_export_symbol_target(current.as_str()) else {
                break;
            };
            if next == current {
                break;
            }
            current = next.to_string();
        }

        current
    }

    pub fn has_local_symbol_target(&self, requested_name: &str) -> bool {
        let target = self.local_symbol_target_name(requested_name);
        self.local_type_symbols.contains_key(&target)
    }

    pub fn local_symbol_dependency_names(&self, symbol_name: &str) -> FxHashSet<String> {
        let mut dependencies = FxHashSet::default();
        let Some(symbol) = self.local_type_symbols.get(symbol_name) else {
            return dependencies;
        };

        for reference in &symbol.dependency_names {
            let root = reference
                .split('.')
                .next()
                .map(str::to_string)
                .unwrap_or_else(|| reference.clone());
            if self.import_locals.contains(&root) {
                continue;
            }
            if self.local_type_symbols.contains_key(&root) && root != symbol_name {
                dependencies.insert(root);
            }
        }

        dependencies
    }

    /// The ORDERED contributor vector of parse-time `RawSourceSurface` raw-fact
    /// records for one `(name, symbol_space)` declared in this file (oracle
    /// harness design item G). A MERGED declaration — same-name interfaces, an
    /// overload group, repeated `declare`s — shares ONE `(name, space)` triple
    /// across several contributors, so the capture retains them as a SOURCE-
    /// ORDER vector (a single-value map would silently drop all but one). Each
    /// contributor's `(ordinal, raw facts)` is INDEPENDENTLY allowlist-checked by
    /// the source-side walk: a single clean contributor does NOT admit the merge
    /// if another carries a REJECT construct (§Q2). Empty slice when nothing was
    /// captured for the triple.
    pub fn raw_source_surfaces_for(
        &self,
        name: &str,
        symbol_space: SymbolSpace,
    ) -> &[RawSourceSurface] {
        self.raw_source_surfaces
            .get(&(name.to_string(), symbol_space))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The FIRST captured contributor for one `(name, symbol_space)`, if any — a
    /// convenience over [`Self::raw_source_surfaces_for`] for the
    /// single-contributor common case. Reads the same ordered vector; callers
    /// that must see EVERY merged contributor use `raw_source_surfaces_for`.
    pub fn raw_source_surface(
        &self,
        name: &str,
        symbol_space: SymbolSpace,
    ) -> Option<&RawSourceSurface> {
        self.raw_source_surfaces
            .get(&(name.to_string(), symbol_space))
            .and_then(|v| v.first())
    }

    /// Enumerate every captured `(name, symbol_space)` contributor, flattening
    /// the ordered per-triple vectors.
    pub fn raw_source_surfaces(
        &self,
    ) -> impl Iterator<Item = ((&str, SymbolSpace), &RawSourceSurface)> {
        self.raw_source_surfaces
            .iter()
            .flat_map(|((name, space), surfaces)| {
                surfaces
                    .iter()
                    .map(move |surface| ((name.as_str(), *space), surface))
            })
    }

    /// Stamp the owning file's canonical id onto every captured raw-fact record.
    /// `analyze_external_type_program` captures without the file context (it sees
    /// only the `Program`); the file-aware artifact-build path supplies it so the
    /// `(canonical, name, symbol_space)` contributor identity is complete.
    pub fn stamp_raw_surface_canonical(&mut self, canonical: &str) {
        for surfaces in self.raw_source_surfaces.values_mut() {
            for surface in surfaces {
                surface.decl_canonical = canonical.to_string();
            }
        }
    }

    pub fn stats(&self) -> AnalyzedExternalTypeSourceStats {
        AnalyzedExternalTypeSourceStats {
            top_level_statement_count: self.top_level_statement_count,
            binding_count: self.extracted.bindings.len(),
            direct_reexport_count: self.extracted.reexport_bindings.len(),
            wildcard_reexport_count: self.extracted.wildcard_reexport_sources.len(),
            import_local_count: self.import_locals.len(),
            local_type_symbol_count: self.local_type_symbols.len(),
            local_export_symbol_count: self.local_export_symbol_targets.len(),
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn analyze_external_type_source(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> AnalyzedExternalTypeSource {
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(allocator, dep_source, source_type).parse();

    if parsed.panicked {
        return AnalyzedExternalTypeSource::default();
    }

    analyze_external_type_program(&parsed.program)
}

pub fn extract_imported_type_bindings(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> ExtractedTypeBindings {
    analyze_external_type_source(dep_source, allocator).extracted
}

pub fn required_import_alias_names_for_binding(
    binding: &ImportedTypeBinding,
    required_import_names: &FxHashSet<String>,
) -> Vec<String> {
    if binding.is_namespace {
        let prefix = format!("{}.", binding.local_name);
        return required_import_names
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .cloned()
            .collect();
    }

    if required_import_names.contains(&binding.local_name) {
        vec![binding.local_name.clone()]
    } else {
        Vec::new()
    }
}

pub fn imported_member_name_for_required_alias(
    binding: &ImportedTypeBinding,
    required_alias_name: &str,
) -> Option<String> {
    if binding.is_namespace {
        let prefix = format!("{}.", binding.local_name);
        return required_alias_name
            .strip_prefix(&prefix)
            .map(str::to_string)
            .filter(|name| !name.is_empty());
    }

    Some(binding.imported_name.clone())
}

/// Lightweight export surface of a file: names that are publicly exported
/// plus wildcard `export *` source specifiers for recursive barrel scanning.
///
/// This does NOT resolve types — it only discovers what names a file exports
/// so the barrel resolution cache can build its `export_map` cheaply.
#[derive(Debug, Clone, Default)]
pub struct ExtractedExportSurface {
    /// Public exported names (type or value).
    /// For `export { Foo as Bar }`, records `Bar` (the public name).
    pub exported_names: FxHashSet<String>,
    /// Source specifiers from `export * from '...'` declarations.
    pub wildcard_reexport_sources: Vec<String>,
}

/// Extract the direct export surface of a source file.
///
/// Collects all publicly exported names and `export *` wildcard sources.
/// This is a lightweight alternative to `extract_imported_type_bindings` —
/// it does not track import bindings or resolve types, only export names.
pub fn extract_export_surface(
    source: &str,
    allocator: &oxc_allocator::Allocator,
) -> ExtractedExportSurface {
    use oxc_ast::ast::*;

    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(allocator, source, source_type).parse();

    if parsed.panicked {
        return ExtractedExportSurface::default();
    }

    let mut result = ExtractedExportSurface::default();

    for stmt in &parsed.program.body {
        match stmt {
            // export interface Foo {} / export type Foo = ... / export enum Foo {} /
            // export class Foo {} / export const Foo = ... / export function Foo()
            Statement::ExportNamedDeclaration(export_decl) => {
                // Named re-export with source: export { X } from './other'
                if export_decl.source.is_some() {
                    for specifier in &export_decl.specifiers {
                        // The public name is `exported`, not `local`
                        result
                            .exported_names
                            .insert(specifier.exported.name().to_string());
                    }
                    continue;
                }

                // Local re-export without source: export { Foo } / export { Foo as Bar }
                if !export_decl.specifiers.is_empty() {
                    for specifier in &export_decl.specifiers {
                        result
                            .exported_names
                            .insert(specifier.exported.name().to_string());
                    }
                    continue;
                }

                // Exported declaration: export interface/type/enum/class/const/function
                if let Some(decl) = &export_decl.declaration {
                    match decl {
                        Declaration::TSInterfaceDeclaration(d) => {
                            result.exported_names.insert(d.id.name.to_string());
                        }
                        Declaration::TSTypeAliasDeclaration(d) => {
                            result.exported_names.insert(d.id.name.to_string());
                        }
                        Declaration::TSEnumDeclaration(d) => {
                            result.exported_names.insert(d.id.name.to_string());
                        }
                        Declaration::ClassDeclaration(d) => {
                            if let Some(id) = &d.id {
                                result.exported_names.insert(id.name.to_string());
                            }
                        }
                        Declaration::FunctionDeclaration(d) => {
                            if let Some(id) = &d.id {
                                result.exported_names.insert(id.name.to_string());
                            }
                        }
                        Declaration::VariableDeclaration(d) => {
                            for declarator in &d.declarations {
                                if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                                    result.exported_names.insert(id.name.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            // export * from './other'
            Statement::ExportAllDeclaration(export_all) => {
                result
                    .wildcard_reexport_sources
                    .push(export_all.source.value.to_string());
            }
            // export default ...
            Statement::ExportDefaultDeclaration(_) => {
                result.exported_names.insert("default".to_string());
            }
            _ => {}
        }
    }

    result
}

pub fn collect_required_import_names_for_external_type(
    type_name: &str,
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> FxHashSet<String> {
    analyze_external_type_source(dep_source, allocator).required_import_names(type_name)
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn analyze_external_type_program(program: &Program<'_>) -> AnalyzedExternalTypeSource {
    let mut result = AnalyzedExternalTypeSource::default();

    for stmt in &program.body {
        result.top_level_statement_count += 1;
        match stmt {
            Statement::ImportDeclaration(import_decl) => {
                let Some(specifiers) = &import_decl.specifiers else {
                    continue;
                };
                for specifier in specifiers {
                    match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(import_spec) => {
                            let local_name = import_spec.local.name.to_string();
                            result.import_locals.insert(local_name.clone());
                            result.extracted.bindings.push(ImportedTypeBinding {
                                local_name,
                                imported_name: import_spec.imported.name().to_string(),
                                source: import_decl.source.value.to_string(),
                                is_namespace: false,
                            });
                            result.local_import_symbol_targets.insert(
                                import_spec.local.name.to_string(),
                                (
                                    import_decl.source.value.to_string(),
                                    import_spec.imported.name().to_string(),
                                ),
                            );
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(import_spec) => {
                            let local_name = import_spec.local.name.to_string();
                            result.import_locals.insert(local_name.clone());
                            result.extracted.bindings.push(ImportedTypeBinding {
                                local_name,
                                imported_name: "default".to_string(),
                                source: import_decl.source.value.to_string(),
                                is_namespace: false,
                            });
                            result.local_import_symbol_targets.insert(
                                import_spec.local.name.to_string(),
                                (import_decl.source.value.to_string(), "default".to_string()),
                            );
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(import_spec) => {
                            result.extracted.bindings.push(ImportedTypeBinding {
                                local_name: import_spec.local.name.to_string(),
                                imported_name: "*".to_string(),
                                source: import_decl.source.value.to_string(),
                                is_namespace: true,
                            });
                        }
                    }
                }
            }
            Statement::ExportNamedDeclaration(export_decl) => {
                if let Some(source) = &export_decl.source {
                    for specifier in &export_decl.specifiers {
                        let local_name = specifier.exported.name().to_string();
                        let imported_name = specifier.local.name().to_string();
                        let binding = ImportedTypeBinding {
                            local_name,
                            imported_name,
                            source: source.value.to_string(),
                            is_namespace: false,
                        };
                        result.extracted.bindings.push(binding.clone());
                        result.extracted.reexport_bindings.push(binding);
                        result.direct_reexport_targets.insert(
                            specifier.exported.name().to_string(),
                            (source.value.to_string(), specifier.local.name().to_string()),
                        );
                    }
                    continue;
                }

                for specifier in &export_decl.specifiers {
                    let Some(imported) = result
                        .extracted
                        .bindings
                        .iter()
                        .find(|binding| specifier.local.name() == binding.local_name)
                    else {
                        continue;
                    };
                    result
                        .extracted
                        .reexport_bindings
                        .push(ImportedTypeBinding {
                            local_name: specifier.exported.name().to_string(),
                            imported_name: imported.imported_name.clone(),
                            source: imported.source.clone(),
                            is_namespace: imported.is_namespace,
                        });
                    result.local_export_symbol_targets.insert(
                        specifier.exported.name().to_string(),
                        specifier.local.name().to_string(),
                    );
                }

                for specifier in &export_decl.specifiers {
                    result
                        .local_export_symbol_targets
                        .entry(specifier.exported.name().to_string())
                        .or_insert_with(|| specifier.local.name().to_string());
                }

                if let Some(declaration) = &export_decl.declaration {
                    record_local_export_symbol_targets_from_declaration(
                        declaration,
                        &mut result.local_export_symbol_targets,
                    );
                    record_local_type_symbol_from_declaration(
                        declaration,
                        &mut result.local_type_symbols,
                    );
                    record_exported_local_type_names_from_declaration(
                        declaration,
                        &mut result.exported_local_type_names,
                    );
                }
            }
            Statement::ExportAllDeclaration(export_all) => {
                result
                    .extracted
                    .wildcard_reexport_sources
                    .push(export_all.source.value.to_string());
            }
            Statement::TSTypeAliasDeclaration(type_alias) => {
                let mut refs = FxHashSet::default();
                let mut structural_refs = FxHashSet::default();
                collect_type_reference_names(&type_alias.type_annotation, &mut refs);
                collect_structural_type_reference_names(
                    &type_alias.type_annotation,
                    StructuralDependencyContext::Root,
                    &mut structural_refs,
                );
                result.local_type_symbols.insert(
                    type_alias.id.name.to_string(),
                    AnalyzedExternalTypeSymbol {
                        kind: AnalyzedExternalTypeSymbolKind::TypeAlias,
                        span: type_alias.span.into(),
                        dependency_names: refs,
                        structural_dependency_names: structural_refs,
                    },
                );
            }
            Statement::TSInterfaceDeclaration(interface) => {
                let mut refs = FxHashSet::default();
                let mut structural_refs = FxHashSet::default();
                for parent in &interface.extends {
                    if let Some(name) = get_expression_reference_name(&parent.expression) {
                        refs.insert(name.clone());
                        structural_refs.insert(name);
                    }
                    if let Some(type_arguments) = &parent.type_arguments {
                        for param in &type_arguments.params {
                            collect_type_reference_names(param, &mut refs);
                            collect_structural_type_reference_names(
                                param,
                                StructuralDependencyContext::Root,
                                &mut structural_refs,
                            );
                        }
                    }
                }
                collect_interface_reference_names(
                    &interface.body.body,
                    &interface.extends,
                    &mut refs,
                );
                collect_structural_interface_reference_names(
                    &interface.body.body,
                    &interface.extends,
                    &mut structural_refs,
                );
                result.local_type_symbols.insert(
                    interface.id.name.to_string(),
                    AnalyzedExternalTypeSymbol {
                        kind: AnalyzedExternalTypeSymbolKind::Interface,
                        span: interface.span.into(),
                        dependency_names: refs,
                        structural_dependency_names: structural_refs,
                    },
                );
            }
            Statement::ClassDeclaration(class_decl) => {
                if let Some(id) = &class_decl.id {
                    let mut refs = FxHashSet::default();
                    let mut structural_refs = FxHashSet::default();
                    collect_class_reference_names(class_decl, &mut refs);
                    collect_structural_class_reference_names(class_decl, &mut structural_refs);
                    result.local_type_symbols.insert(
                        id.name.to_string(),
                        AnalyzedExternalTypeSymbol {
                            kind: AnalyzedExternalTypeSymbolKind::Class,
                            span: class_decl.span.into(),
                            dependency_names: refs,
                            structural_dependency_names: structural_refs,
                        },
                    );
                }
            }
            Statement::ExportDefaultDeclaration(export_default) => {
                match &export_default.declaration {
                    ExportDefaultDeclarationKind::ClassDeclaration(class_decl) => {
                        let mut refs = FxHashSet::default();
                        let mut structural_refs = FxHashSet::default();
                        collect_class_reference_names(class_decl, &mut refs);
                        collect_structural_class_reference_names(class_decl, &mut structural_refs);
                        result.local_type_symbols.insert(
                            "default".to_string(),
                            AnalyzedExternalTypeSymbol {
                                kind: AnalyzedExternalTypeSymbolKind::Class,
                                span: export_default.span.into(),
                                dependency_names: refs,
                                structural_dependency_names: structural_refs,
                            },
                        );
                    }
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                        let mut refs = FxHashSet::default();
                        let mut structural_refs = FxHashSet::default();
                        for parent in &interface.extends {
                            if let Some(name) = get_expression_reference_name(&parent.expression) {
                                refs.insert(name.clone());
                                structural_refs.insert(name);
                            }
                            if let Some(type_arguments) = &parent.type_arguments {
                                for param in &type_arguments.params {
                                    collect_type_reference_names(param, &mut refs);
                                    collect_structural_type_reference_names(
                                        param,
                                        StructuralDependencyContext::Root,
                                        &mut structural_refs,
                                    );
                                }
                            }
                        }
                        collect_interface_reference_names(
                            &interface.body.body,
                            &interface.extends,
                            &mut refs,
                        );
                        collect_structural_interface_reference_names(
                            &interface.body.body,
                            &interface.extends,
                            &mut structural_refs,
                        );
                        result.local_type_symbols.insert(
                            "default".to_string(),
                            AnalyzedExternalTypeSymbol {
                                kind: AnalyzedExternalTypeSymbolKind::Interface,
                                span: export_default.span.into(),
                                dependency_names: refs,
                                structural_dependency_names: structural_refs,
                            },
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Oracle harness design item G: capture the parse-time `RawSourceSurface`
    // raw-fact inventory while the OXC arena is still live (same parse pass that
    // built the shallow inventory above). Overload groups merge by name so a
    // multi-signature `function f` surfaces its overload-SET arity.
    let captured = merge_overload_groups(
        program
            .body
            .iter()
            .flat_map(capture_statement_surfaces)
            .collect(),
    );
    for c in captured {
        // Append in source order: a MERGED declaration shares one `(name, space)`
        // triple across several contributors, so each is RETAINED (not last-wins
        // overwritten) for the source-side walk's per-contributor allowlist check.
        result
            .raw_source_surfaces
            .entry((c.name, c.symbol_space))
            .or_default()
            .push(c.surface);
    }

    result
}

fn record_local_type_symbol_from_declaration(
    declaration: &Declaration<'_>,
    local_type_symbols: &mut FxHashMap<String, AnalyzedExternalTypeSymbol>,
) {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => {
            let mut refs = FxHashSet::default();
            let mut structural_refs = FxHashSet::default();
            collect_type_reference_names(&type_alias.type_annotation, &mut refs);
            collect_structural_type_reference_names(
                &type_alias.type_annotation,
                StructuralDependencyContext::Root,
                &mut structural_refs,
            );
            local_type_symbols.insert(
                type_alias.id.name.to_string(),
                AnalyzedExternalTypeSymbol {
                    kind: AnalyzedExternalTypeSymbolKind::TypeAlias,
                    span: type_alias.span.into(),
                    dependency_names: refs,
                    structural_dependency_names: structural_refs,
                },
            );
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            let mut refs = FxHashSet::default();
            let mut structural_refs = FxHashSet::default();
            for parent in &interface.extends {
                if let Some(name) = get_expression_reference_name(&parent.expression) {
                    refs.insert(name.clone());
                    structural_refs.insert(name);
                }
                if let Some(type_arguments) = &parent.type_arguments {
                    for param in &type_arguments.params {
                        collect_type_reference_names(param, &mut refs);
                        collect_structural_type_reference_names(
                            param,
                            StructuralDependencyContext::Root,
                            &mut structural_refs,
                        );
                    }
                }
            }
            collect_interface_reference_names(&interface.body.body, &interface.extends, &mut refs);
            collect_structural_interface_reference_names(
                &interface.body.body,
                &interface.extends,
                &mut structural_refs,
            );
            local_type_symbols.insert(
                interface.id.name.to_string(),
                AnalyzedExternalTypeSymbol {
                    kind: AnalyzedExternalTypeSymbolKind::Interface,
                    span: interface.span.into(),
                    dependency_names: refs,
                    structural_dependency_names: structural_refs,
                },
            );
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                let mut refs = FxHashSet::default();
                let mut structural_refs = FxHashSet::default();
                collect_class_reference_names(class_decl, &mut refs);
                collect_structural_class_reference_names(class_decl, &mut structural_refs);
                local_type_symbols.insert(
                    id.name.to_string(),
                    AnalyzedExternalTypeSymbol {
                        kind: AnalyzedExternalTypeSymbolKind::Class,
                        span: class_decl.span.into(),
                        dependency_names: refs,
                        structural_dependency_names: structural_refs,
                    },
                );
            }
        }
        _ => {}
    }
}

fn record_exported_local_type_names_from_declaration(
    declaration: &Declaration<'_>,
    exported_local_type_names: &mut FxHashSet<String>,
) {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => {
            exported_local_type_names.insert(type_alias.id.name.to_string());
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            exported_local_type_names.insert(interface.id.name.to_string());
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                exported_local_type_names.insert(id.name.to_string());
            }
        }
        _ => {}
    }
}

fn record_local_export_symbol_targets_from_declaration(
    declaration: &Declaration<'_>,
    local_export_symbol_targets: &mut FxHashMap<String, String>,
) {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => {
            local_export_symbol_targets
                .entry(type_alias.id.name.to_string())
                .or_insert_with(|| type_alias.id.name.to_string());
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            local_export_symbol_targets
                .entry(interface.id.name.to_string())
                .or_insert_with(|| interface.id.name.to_string());
        }
        Declaration::TSEnumDeclaration(enum_decl) => {
            local_export_symbol_targets
                .entry(enum_decl.id.name.to_string())
                .or_insert_with(|| enum_decl.id.name.to_string());
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                local_export_symbol_targets
                    .entry(id.name.to_string())
                    .or_insert_with(|| id.name.to_string());
            }
        }
        Declaration::FunctionDeclaration(function_decl) => {
            if let Some(id) = &function_decl.id {
                local_export_symbol_targets
                    .entry(id.name.to_string())
                    .or_insert_with(|| id.name.to_string());
            }
        }
        Declaration::VariableDeclaration(variable_decl) => {
            for declarator in &variable_decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    continue;
                };
                local_export_symbol_targets
                    .entry(id.name.to_string())
                    .or_insert_with(|| id.name.to_string());
            }
        }
        _ => {}
    }
}

fn collect_named_import_locals(program: &Program<'_>) -> FxHashSet<String> {
    let mut locals = FxHashSet::default();
    for stmt in &program.body {
        let Statement::ImportDeclaration(import_decl) = stmt else {
            continue;
        };
        let Some(specifiers) = &import_decl.specifiers else {
            continue;
        };
        for specifier in specifiers {
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(import_spec) => {
                    locals.insert(import_spec.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(import_spec) => {
                    locals.insert(import_spec.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(_) => {}
            }
        }
    }
    locals
}

fn enqueue_required_import_refs(
    refs: FxHashSet<String>,
    import_locals: &FxHashSet<String>,
    required_imports: &mut FxHashSet<String>,
    pending: &mut Vec<String>,
    visited: &FxHashSet<String>,
) {
    for reference in refs {
        let root = reference
            .split('.')
            .next()
            .map(str::to_string)
            .unwrap_or(reference);
        if import_locals.contains(&root) {
            required_imports.insert(root);
        } else if !visited.contains(&root) {
            pending.push(root);
        }
    }
}

fn collect_interface_reference_names(
    members: &[TSSignature],
    heritage: &[TSInterfaceHeritage],
    refs: &mut FxHashSet<String>,
) {
    for h in heritage {
        if let Expression::Identifier(id) = &h.expression {
            refs.insert(id.name.to_string());
        }
        if let Some(type_arguments) = &h.type_arguments {
            for param in &type_arguments.params {
                collect_type_reference_names(param, refs);
            }
        }
    }

    for member in members {
        match member {
            TSSignature::TSPropertySignature(prop) => {
                if let Some(type_annotation) = &prop.type_annotation {
                    collect_type_reference_names(&type_annotation.type_annotation, refs);
                }
            }
            TSSignature::TSMethodSignature(method) => {
                collect_formal_parameter_reference_names(&method.params, refs);
            }
            TSSignature::TSCallSignatureDeclaration(call) => {
                collect_formal_parameter_reference_names(&call.params, refs);
            }
            TSSignature::TSIndexSignature(index) => {
                collect_type_reference_names(&index.type_annotation.type_annotation, refs);
            }
            TSSignature::TSConstructSignatureDeclaration(_) => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuralDependencyContext {
    Root,
    CallableParam,
    LeafProperty,
}

fn collect_structural_interface_reference_names(
    members: &[TSSignature],
    heritage: &[TSInterfaceHeritage],
    refs: &mut FxHashSet<String>,
) {
    for h in heritage {
        if let Expression::Identifier(id) = &h.expression {
            refs.insert(id.name.to_string());
        }
        if let Some(type_arguments) = &h.type_arguments {
            for param in &type_arguments.params {
                collect_structural_type_reference_names(
                    param,
                    StructuralDependencyContext::Root,
                    refs,
                );
            }
        }
    }

    for member in members {
        match member {
            TSSignature::TSPropertySignature(prop) => {
                if let Some(type_annotation) = &prop.type_annotation {
                    collect_structural_type_reference_names(
                        &type_annotation.type_annotation,
                        StructuralDependencyContext::LeafProperty,
                        refs,
                    );
                }
            }
            TSSignature::TSMethodSignature(method) => {
                collect_structural_formal_parameter_reference_names(
                    &method.params,
                    StructuralDependencyContext::CallableParam,
                    refs,
                );
            }
            TSSignature::TSCallSignatureDeclaration(call) => {
                collect_structural_formal_parameter_reference_names(
                    &call.params,
                    StructuralDependencyContext::CallableParam,
                    refs,
                );
            }
            TSSignature::TSIndexSignature(index) => {
                collect_structural_type_reference_names(
                    &index.type_annotation.type_annotation,
                    StructuralDependencyContext::LeafProperty,
                    refs,
                );
            }
            TSSignature::TSConstructSignatureDeclaration(_) => {}
        }
    }
}

fn collect_class_reference_names(class: &Class<'_>, refs: &mut FxHashSet<String>) {
    if let Some(super_class) = &class.super_class {
        if let Some(name) = get_expression_reference_name(super_class) {
            refs.insert(name);
        }
        if let Some(type_args) = &class.super_type_arguments {
            for param in &type_args.params {
                collect_type_reference_names(param, refs);
            }
        }
    }

    for clause in &class.implements {
        refs.insert(get_type_reference_name(&clause.expression));
        if let Some(type_args) = &clause.type_arguments {
            for param in &type_args.params {
                collect_type_reference_names(param, refs);
            }
        }
    }

    for member in &class.body.body {
        match member {
            ClassElement::PropertyDefinition(prop) => {
                if let Some(type_annotation) = &prop.type_annotation {
                    collect_type_reference_names(&type_annotation.type_annotation, refs);
                }
            }
            ClassElement::MethodDefinition(method) => {
                collect_formal_parameter_reference_names(&method.value.params, refs);
            }
            ClassElement::AccessorProperty(prop) => {
                if let Some(type_annotation) = &prop.type_annotation {
                    collect_type_reference_names(&type_annotation.type_annotation, refs);
                }
            }
            ClassElement::TSIndexSignature(sig) => {
                collect_type_reference_names(&sig.type_annotation.type_annotation, refs);
            }
            _ => {}
        }
    }
}

fn collect_structural_class_reference_names(class: &Class<'_>, refs: &mut FxHashSet<String>) {
    if let Some(super_class) = &class.super_class {
        if let Some(name) = get_expression_reference_name(super_class) {
            refs.insert(name);
        }
        if let Some(type_args) = &class.super_type_arguments {
            for param in &type_args.params {
                collect_structural_type_reference_names(
                    param,
                    StructuralDependencyContext::Root,
                    refs,
                );
            }
        }
    }

    for clause in &class.implements {
        refs.insert(get_type_reference_name(&clause.expression));
        if let Some(type_args) = &clause.type_arguments {
            for param in &type_args.params {
                collect_structural_type_reference_names(
                    param,
                    StructuralDependencyContext::Root,
                    refs,
                );
            }
        }
    }

    for member in &class.body.body {
        match member {
            ClassElement::PropertyDefinition(prop) => {
                if let Some(type_annotation) = &prop.type_annotation {
                    collect_structural_type_reference_names(
                        &type_annotation.type_annotation,
                        StructuralDependencyContext::LeafProperty,
                        refs,
                    );
                }
            }
            ClassElement::MethodDefinition(method) => {
                collect_structural_formal_parameter_reference_names(
                    &method.value.params,
                    StructuralDependencyContext::CallableParam,
                    refs,
                );
            }
            ClassElement::AccessorProperty(prop) => {
                if let Some(type_annotation) = &prop.type_annotation {
                    collect_structural_type_reference_names(
                        &type_annotation.type_annotation,
                        StructuralDependencyContext::LeafProperty,
                        refs,
                    );
                }
            }
            ClassElement::TSIndexSignature(sig) => {
                collect_structural_type_reference_names(
                    &sig.type_annotation.type_annotation,
                    StructuralDependencyContext::LeafProperty,
                    refs,
                );
            }
            _ => {}
        }
    }
}

pub(super) fn collect_type_reference_names(ts_type: &TSType<'_>, refs: &mut FxHashSet<String>) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            refs.insert(get_type_reference_name(&type_ref.type_name));
            if let Some(params) = &type_ref.type_arguments {
                for param in &params.params {
                    collect_type_reference_names(param, refs);
                }
            }
        }
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                collect_type_reference_names(ty, refs);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                collect_type_reference_names(ty, refs);
            }
        }
        TSType::TSTypeLiteral(literal) => {
            collect_interface_reference_names(&literal.members, &[], refs);
        }
        TSType::TSArrayType(array) => {
            collect_type_reference_names(&array.element_type, refs);
        }
        TSType::TSTupleType(tuple) => {
            for element in &tuple.element_types {
                match element {
                    TSTupleElement::TSOptionalType(optional) => {
                        collect_type_reference_names(&optional.type_annotation, refs);
                    }
                    TSTupleElement::TSRestType(rest) => {
                        collect_type_reference_names(&rest.type_annotation, refs);
                    }
                    TSTupleElement::TSNamedTupleMember(named) => {
                        if let Some(ts_type) = named.element_type.as_ts_type() {
                            collect_type_reference_names(ts_type, refs);
                        }
                    }
                    _ => {
                        if let Some(ts_type) = element.as_ts_type() {
                            collect_type_reference_names(ts_type, refs);
                        }
                    }
                }
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_type_reference_names(&cond.check_type, refs);
            collect_type_reference_names(&cond.extends_type, refs);
            collect_type_reference_names(&cond.true_type, refs);
            collect_type_reference_names(&cond.false_type, refs);
        }
        TSType::TSMappedType(mapped) => {
            collect_type_reference_names(&mapped.constraint, refs);
            if let Some(type_annotation) = &mapped.type_annotation {
                collect_type_reference_names(type_annotation, refs);
            }
        }
        TSType::TSIndexedAccessType(indexed) => {
            collect_type_reference_names(&indexed.object_type, refs);
            collect_type_reference_names(&indexed.index_type, refs);
        }
        TSType::TSTypeOperatorType(operator) => {
            collect_type_reference_names(&operator.type_annotation, refs);
        }
        TSType::TSParenthesizedType(paren) => {
            collect_type_reference_names(&paren.type_annotation, refs);
        }
        TSType::TSTemplateLiteralType(template) => {
            for ty in &template.types {
                collect_type_reference_names(ty, refs);
            }
        }
        TSType::TSFunctionType(function) => {
            collect_formal_parameter_reference_names(&function.params, refs);
        }
        TSType::TSConstructorType(constructor) => {
            collect_formal_parameter_reference_names(&constructor.params, refs);
        }
        TSType::TSTypeQuery(query) => {
            if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                refs.insert(ident.name.to_string());
            }
        }
        _ => {}
    }
}

fn collect_structural_type_reference_names(
    ts_type: &TSType<'_>,
    context: StructuralDependencyContext,
    refs: &mut FxHashSet<String>,
) {
    match ts_type {
        TSType::TSTypeReference(type_ref) => {
            if matches!(
                context,
                StructuralDependencyContext::Root | StructuralDependencyContext::CallableParam
            ) {
                refs.insert(get_type_reference_name(&type_ref.type_name));
                if let Some(params) = &type_ref.type_arguments {
                    for param in &params.params {
                        collect_structural_type_reference_names(param, context, refs);
                    }
                }
            }
        }
        TSType::TSUnionType(union) => {
            for ty in &union.types {
                collect_structural_type_reference_names(ty, context, refs);
            }
        }
        TSType::TSIntersectionType(intersection) => {
            for ty in &intersection.types {
                collect_structural_type_reference_names(ty, context, refs);
            }
        }
        TSType::TSTypeLiteral(literal) => {
            collect_structural_interface_reference_names(&literal.members, &[], refs);
        }
        TSType::TSArrayType(array) => {
            collect_structural_type_reference_names(&array.element_type, context, refs);
        }
        TSType::TSTupleType(tuple) => {
            for element in &tuple.element_types {
                match element {
                    TSTupleElement::TSOptionalType(optional) => {
                        collect_structural_type_reference_names(
                            &optional.type_annotation,
                            context,
                            refs,
                        );
                    }
                    TSTupleElement::TSRestType(rest) => {
                        collect_structural_type_reference_names(
                            &rest.type_annotation,
                            context,
                            refs,
                        );
                    }
                    TSTupleElement::TSNamedTupleMember(named) => {
                        if let Some(ts_type) = named.element_type.as_ts_type() {
                            collect_structural_type_reference_names(ts_type, context, refs);
                        }
                    }
                    _ => {
                        if let Some(ts_type) = element.as_ts_type() {
                            collect_structural_type_reference_names(ts_type, context, refs);
                        }
                    }
                }
            }
        }
        TSType::TSConditionalType(cond) => {
            collect_structural_type_reference_names(&cond.check_type, context, refs);
            collect_structural_type_reference_names(&cond.extends_type, context, refs);
            collect_structural_type_reference_names(&cond.true_type, context, refs);
            collect_structural_type_reference_names(&cond.false_type, context, refs);
        }
        TSType::TSMappedType(mapped) => {
            collect_structural_type_reference_names(&mapped.constraint, context, refs);
            if let Some(type_annotation) = &mapped.type_annotation {
                collect_structural_type_reference_names(type_annotation, context, refs);
            }
        }
        TSType::TSIndexedAccessType(indexed) => {
            collect_structural_type_reference_names(
                &indexed.object_type,
                StructuralDependencyContext::Root,
                refs,
            );
            collect_structural_type_reference_names(
                &indexed.index_type,
                StructuralDependencyContext::Root,
                refs,
            );
        }
        TSType::TSTypeOperatorType(operator) => {
            collect_structural_type_reference_names(&operator.type_annotation, context, refs);
        }
        TSType::TSParenthesizedType(paren) => {
            collect_structural_type_reference_names(&paren.type_annotation, context, refs);
        }
        TSType::TSTemplateLiteralType(template) => {
            for ty in &template.types {
                collect_structural_type_reference_names(ty, context, refs);
            }
        }
        TSType::TSFunctionType(function) => {
            if context != StructuralDependencyContext::LeafProperty {
                collect_structural_formal_parameter_reference_names(
                    &function.params,
                    StructuralDependencyContext::CallableParam,
                    refs,
                );
            }
        }
        TSType::TSConstructorType(constructor) => {
            if context != StructuralDependencyContext::LeafProperty {
                collect_structural_formal_parameter_reference_names(
                    &constructor.params,
                    StructuralDependencyContext::CallableParam,
                    refs,
                );
            }
        }
        TSType::TSTypeQuery(query) => {
            if matches!(
                context,
                StructuralDependencyContext::Root | StructuralDependencyContext::CallableParam
            ) {
                if let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name {
                    refs.insert(ident.name.to_string());
                }
            }
        }
        _ => {}
    }
}

fn collect_formal_parameter_reference_names(
    params: &FormalParameters<'_>,
    refs: &mut FxHashSet<String>,
) {
    // Component-meta only needs callable parameter surfaces for props/emits/slots.
    // Skipping return-type-only imports avoids pulling large framework graphs
    // like `VNode` into companion/source-merge work.
    for param in &params.items {
        if let Some(type_annotation) = &param.type_annotation {
            collect_type_reference_names(&type_annotation.type_annotation, refs);
        }
    }
}

fn collect_structural_formal_parameter_reference_names(
    params: &FormalParameters<'_>,
    context: StructuralDependencyContext,
    refs: &mut FxHashSet<String>,
) {
    for param in &params.items {
        if let Some(type_annotation) = &param.type_annotation {
            collect_structural_type_reference_names(
                &type_annotation.type_annotation,
                context,
                refs,
            );
        }
    }
}

pub fn resolve_external_type_with_companion(
    type_name: &str,
    dep_source: &str,
    companion_types: &FxHashMap<String, ResolvedElements>,
    allocator: &oxc_allocator::Allocator,
) -> Option<ResolvedElements> {
    resolve_external_type_with_companion_and_canonical(
        type_name,
        dep_source,
        companion_types,
        allocator,
        "",
    )
}

/// Like `resolve_external_type_with_companion` but stamps `type_expr_scope`
/// with the supplied canonical_id on every populated `type_expr` in the
/// returned `ResolvedElements`.
pub fn resolve_external_type_with_companion_and_canonical(
    type_name: &str,
    dep_source: &str,
    companion_types: &FxHashMap<String, ResolvedElements>,
    allocator: &oxc_allocator::Allocator,
    external_canonical_id: &str,
) -> Option<ResolvedElements> {
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(allocator, dep_source, source_type).parse();

    if parsed.panicked {
        return None;
    }

    let analysis = analyze_external_type_program(&parsed.program);
    let base_ctx = build_type_context(&parsed.program, dep_source.as_bytes(), 0);
    resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical(
        type_name,
        &parsed.program,
        dep_source.as_bytes(),
        &base_ctx,
        &analysis,
        companion_types,
        external_canonical_id,
    )
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_external_type_in_program_with_analyzed_symbol_companion(
    type_name: &str,
    program: &Program<'_>,
    source_bytes: &[u8],
    analysis: &AnalyzedExternalTypeSource,
    imported_companions: &FxHashMap<String, ResolvedElements>,
) -> Option<ResolvedElements> {
    resolve_external_type_in_program_with_analyzed_symbol_companion_and_canonical(
        type_name,
        program,
        source_bytes,
        analysis,
        imported_companions,
        "",
    )
}

/// Like `resolve_external_type_in_program_with_analyzed_symbol_companion`
/// but stamps `type_expr_scope` on every populated `type_expr` with the
/// supplied canonical_id.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_external_type_in_program_with_analyzed_symbol_companion_and_canonical(
    type_name: &str,
    program: &Program<'_>,
    source_bytes: &[u8],
    analysis: &AnalyzedExternalTypeSource,
    imported_companions: &FxHashMap<String, ResolvedElements>,
    external_canonical_id: &str,
) -> Option<ResolvedElements> {
    let base_ctx = build_type_context(program, source_bytes, 0);
    resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical(
        type_name,
        program,
        source_bytes,
        &base_ctx,
        analysis,
        imported_companions,
        external_canonical_id,
    )
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_external_type_in_context_with_analyzed_symbol_companion<'ctx, 'a: 'ctx>(
    type_name: &str,
    program: &'ctx Program<'a>,
    source_bytes: &[u8],
    base_ctx: &TypeResolutionContext<'ctx, 'a>,
    analysis: &AnalyzedExternalTypeSource,
    imported_companions: &FxHashMap<String, ResolvedElements>,
) -> Option<ResolvedElements> {
    resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical(
        type_name,
        program,
        source_bytes,
        base_ctx,
        analysis,
        imported_companions,
        "",
    )
}

/// Like `resolve_external_type_in_context_with_analyzed_symbol_companion`
/// but stamps `type_expr_scope` on every populated `type_expr` with the
/// supplied canonical_id.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical<
    'ctx,
    'a: 'ctx,
>(
    type_name: &str,
    program: &'ctx Program<'a>,
    source_bytes: &[u8],
    base_ctx: &TypeResolutionContext<'ctx, 'a>,
    analysis: &AnalyzedExternalTypeSource,
    imported_companions: &FxHashMap<String, ResolvedElements>,
    external_canonical_id: &str,
) -> Option<ResolvedElements> {
    if type_name != "default" && !analysis.has_local_symbol_target(type_name) {
        return resolve_value_declaration_type(type_name, program, source_bytes, 0, base_ctx).map(
            |resolved| {
                finalize_external_resolution_with_offset(
                    resolved,
                    source_bytes,
                    0,
                    external_canonical_id,
                )
            },
        );
    }

    let target_name = analysis.local_symbol_target_name(type_name);
    resolve_external_type_in_context_with_companion(
        target_name.as_str(),
        program,
        source_bytes,
        base_ctx,
        imported_companions,
        external_canonical_id,
    )
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_external_type_in_context_with_companion<'ctx, 'a: 'ctx>(
    type_name: &str,
    program: &'ctx Program<'a>,
    source_bytes: &[u8],
    base_ctx: &TypeResolutionContext<'ctx, 'a>,
    imported_companions: &FxHashMap<String, ResolvedElements>,
    external_canonical_id: &str,
) -> Option<ResolvedElements> {
    let mut ctx = base_ctx.clone();
    ctx.extend_companion_types(imported_companions);

    let mut result = resolve_named_external_type(type_name, program, source_bytes, &ctx);

    if result.is_none() && type_name == "default" {
        result = resolve_default_exported_type(program, &ctx);
    }

    result.map(|resolved| {
        finalize_external_resolution_with_offset(resolved, source_bytes, 0, external_canonical_id)
    })
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn resolve_named_external_type<'ctx, 'a: 'ctx>(
    type_name: &str,
    program: &'ctx Program<'a>,
    source_bytes: &[u8],
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<ResolvedElements> {
    // Check per-surface type blocklist before expanding
    if ctx.is_type_blocked(type_name) {
        return Some(ResolvedElements::default());
    }

    // External type resolution is reached when a macro T resolves a
    // cross-file type (`defineProps<Foo>()` where `Foo` is imported).
    // The macro entry IS the named type's body, so dispatch with
    // `from_root_body = true`. Heritage descent inside the named
    // resolution flips to `false` internally.
    let mut guard = vec![type_name.to_string()];
    if let Some(resolved) =
        resolve_named_local_type_with_ctx_ref(type_name, None, 0, ctx, true, &mut guard)
    {
        return Some((*resolved).clone());
    }

    resolve_value_declaration_type(type_name, program, source_bytes, 0, ctx)
}

fn resolve_default_exported_type<'ctx, 'a: 'ctx>(
    program: &'ctx Program<'a>,
    ctx: &TypeResolutionContext<'ctx, 'a>,
) -> Option<ResolvedElements> {
    for stmt in &program.body {
        let Statement::ExportDefaultDeclaration(export) = stmt else {
            continue;
        };

        match &export.declaration {
            ExportDefaultDeclarationKind::ClassDeclaration(class_decl) => {
                let guard_name = class_decl
                    .id
                    .as_ref()
                    .map(|id| id.name.to_string())
                    .unwrap_or_else(|| "default".to_string());
                let mut guard = vec![guard_name.clone()];
                if let Some(id) = &class_decl.id {
                    if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
                        id.name.as_str(),
                        None,
                        0,
                        ctx,
                        true,
                        &mut guard,
                    ) {
                        return Some((*resolved).clone());
                    }
                }

                let mut resolved = ResolvedElements::default();
                let mut guard = vec![guard_name];
                resolve_class_with_heritage_ctx_ref(
                    class_decl,
                    0,
                    &mut resolved,
                    ctx,
                    true,
                    &mut guard,
                );
                resolved.root_runtime_types = vec![RuntimeType::Object];
                return Some(resolved);
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface_decl) => {
                let mut guard = vec![interface_decl.id.name.to_string()];
                if let Some(resolved) = resolve_named_local_type_with_ctx_ref(
                    interface_decl.id.name.as_str(),
                    None,
                    0,
                    ctx,
                    true,
                    &mut guard,
                ) {
                    return Some((*resolved).clone());
                }

                let mut resolved = ResolvedElements::default();
                let extends = extract_heritage_type_names(&interface_decl.extends);
                let mut guard = vec![interface_decl.id.name.to_string()];
                resolve_interface_with_extends_ctx_ref(
                    &interface_decl.body.body,
                    &extends,
                    &interface_decl.extends,
                    0,
                    &mut resolved,
                    ctx,
                    true,
                    &mut guard,
                );
                resolved.root_runtime_types = vec![RuntimeType::Object];
                return Some(resolved);
            }
            _ => {}
        }
    }

    None
}

fn finalize_external_resolution_with_offset(
    mut resolved: ResolvedElements,
    source_bytes: &[u8],
    span_offset: u32,
    external_canonical_id: &str,
) -> ResolvedElements {
    // Stamp the external file's canonical_id as `type_expr_scope` on every
    // populated `type_expr`. The lowering itself happened at construction
    // time inside the external file's parse arena (elements.rs / decl.rs).
    // Stamping here completes the pairing invariant
    // `type_expr.is_some() <=> type_expr_scope.is_some()` for the external
    // path before the result leaves the parser.
    let scope = TypeExprScope::new(external_canonical_id);
    resolved.stamp_type_expr_scope(&scope);
    debug_assert!(
        resolved.assert_typed_form_populated().is_ok(),
        "finalize_external_resolution_with_offset must satisfy the typed-form pairing invariant: {}",
        resolved
            .assert_typed_form_populated()
            .err()
            .unwrap_or_default()
    );
    for prop in &mut resolved.props {
        let start = prop.key.start as usize;
        let end = prop.key.end as usize;
        if prop.key_name.is_none() && start < end && end <= source_bytes.len() {
            if let Ok(name) = std::str::from_utf8(&source_bytes[start..end]) {
                prop.key_name = Some(name.to_string());
            }
        }
        // Set type_text from type_span for cross-file props (spans reference
        // this external source, not the consuming SFC). Skip if already set
        // by a previous resolution step (e.g., companion-derived props).
        if prop.type_text.is_none() {
            if let Some(type_span) = prop.type_span {
                let ts = type_span.start as usize;
                let te = type_span.end as usize;
                if ts < te && te <= source_bytes.len() {
                    if let Ok(text) = std::str::from_utf8(&source_bytes[ts..te]) {
                        prop.type_text = Some(text.to_string());
                    }
                }
            }
        }
        prop.span = Span::new(
            prop.span.start.saturating_add(span_offset),
            prop.span.end.saturating_add(span_offset),
        );
        prop.key = Span::new(
            prop.key.start.saturating_add(span_offset),
            prop.key.end.saturating_add(span_offset),
        );
        prop.type_span = prop.type_span.map(|type_span| {
            Span::new(
                type_span.start.saturating_add(span_offset),
                type_span.end.saturating_add(span_offset),
            )
        });
        prop.map_local = false;
        prop.span_is_absolute = false;
    }
    for emit in &mut resolved.emits {
        emit.span = Span::new(
            emit.span.start.saturating_add(span_offset),
            emit.span.end.saturating_add(span_offset),
        );
        emit.name_span = emit.name_span.map(|name_span| {
            Span::new(
                name_span.start.saturating_add(span_offset),
                name_span.end.saturating_add(span_offset),
            )
        });
        emit.map_local = false;
        emit.span_is_absolute = false;
    }

    resolved
}

/// Hash the resolved type shape for cache comparison (SHA-256, truncated to 16 bytes).
///
/// Produces a stable hash from prop names + runtime types + optional flags + emits.
/// Two different source texts that resolve to the same prop shape produce the same hash.
///
/// # Arguments
/// * `resolved` - The resolved type elements
/// * `source` - Source bytes needed to extract prop key names from spans
pub fn hash_resolved_type(resolved: &ResolvedElements, source: &[u8]) -> [u8; 16] {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();

    // Hash props sorted by key name for stability
    let mut props: Vec<_> = resolved
        .props
        .iter()
        .map(|p| {
            let key_name = &source[p.key.start as usize..p.key.end as usize];
            let mut runtime_types: Vec<&str> = p.types.iter().map(|t| t.as_str()).collect();
            runtime_types.sort();
            (key_name, runtime_types, p.optional)
        })
        .collect();
    props.sort_by_key(|(name, _, _)| *name);

    hasher.update((props.len() as u32).to_le_bytes());
    for (name, types, optional) in &props {
        hasher.update((name.len() as u32).to_le_bytes());
        hasher.update(name);
        hasher.update((types.len() as u32).to_le_bytes());
        for t in types {
            hasher.update(t.as_bytes());
        }
        hasher.update([*optional as u8]);
    }

    // Hash emits sorted by name
    let mut emits: Vec<&str> = resolved.emits.iter().map(|e| e.name.as_str()).collect();
    emits.sort();

    hasher.update((emits.len() as u32).to_le_bytes());
    for name in &emits {
        hasher.update(name.as_bytes());
    }

    hasher.update([resolved.has_call_signature as u8]);

    let hash = hasher.finalize();
    let mut result = [0u8; 16];
    result.copy_from_slice(&hash[..16]);
    result
}
