//! Framework-neutral script routing and declaration inventory.
//!
//! Captures imports, exports, local declaration headers, and structural
//! dependency names from one parsed program. It performs no type resolution
//! and produces no compiler-facing semantic surface.

use std::str;

use oxc_ast::ast::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::common::Span;
use crate::utils::oxc::script::raw_surface::{
    capture_statement_surfaces, merge_overload_groups, RawSourceSurface, SymbolSpace,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTypeBinding {
    pub local_name: String,
    pub imported_name: String,
    pub source: String,
    pub is_namespace: bool,
}

/// Result of extracting type bindings from a dependency file.
/// Includes named bindings (from `import` and `export {} from`),
/// wildcard re-export sources (from `export * from`), and bindingless
/// import sources (from `import './x'` / `import {} from './x'`).
#[derive(Debug, Clone, Default)]
pub struct ExtractedTypeBindings {
    pub bindings: Vec<ImportedTypeBinding>,
    pub reexport_bindings: Vec<ImportedTypeBinding>,
    pub wildcard_reexport_sources: Vec<String>,
    /// Import declarations that bind NO local name — side-effect imports
    /// (`import './x'`) and empty named-import lists (`import {} from
    /// './x'`). They still create a cross-file dependency edge (the
    /// specifier resolves to a canonical file), so the shallow edge
    /// inventory must retain them: edge-currency oracles treat any
    /// cross-file edge as dependency-set-derived state that can go stale
    /// when the file set moves. In declaration order.
    pub bindingless_import_sources: Vec<String>,
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
    /// The local value name a CommonJS-style `export = X` assigns the whole
    /// module to, when present. `typeof import("./m")` against such a module
    /// resolves to `typeof X` (the export-assignment value), not an object
    /// wrapping the named exports. `None` for an ordinary ESM module.
    export_assignment_target: Option<String>,
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

    /// The local value name a CommonJS `export = X` assigns the whole module
    /// to (`Some("X")`), or `None` for an ordinary ESM module.
    pub fn export_assignment_target(&self) -> Option<&str> {
        self.export_assignment_target.as_deref()
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
    /// harness). A MERGED declaration — same-name interfaces, an
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
    analyze_external_type_program_impl(program, true)
}

/// HEADER-ONLY analyzer variant — the `IndexedReady` artifact producer.
///
/// Identical import/export/reexport/symbol-NAME inventory to
/// [`analyze_external_type_program`], with the per-declaration BODY walks
/// skipped: `local_type_symbols` carry kind + span with EMPTY dependency
/// sets, and no `RawSourceSurface` inventory is captured. Body-derived
/// facts (dependency names, raw surfaces) are demand products of the lazy
/// declaration-body path ([`collect_statement_dependency_names`], the
/// per-statement raw-surface capture) — never an eager whole-program walk
/// at artifact publish.
pub fn analyze_external_type_program_headers(program: &Program<'_>) -> AnalyzedExternalTypeSource {
    analyze_external_type_program_impl(program, false)
}

fn analyze_external_type_program_impl(
    program: &Program<'_>,
    with_bodies: bool,
) -> AnalyzedExternalTypeSource {
    let mut result = AnalyzedExternalTypeSource::default();

    for stmt in &program.body {
        result.top_level_statement_count += 1;
        match stmt {
            Statement::ImportDeclaration(import_decl) => {
                let Some(specifiers) = &import_decl.specifiers else {
                    // Side-effect import (`import './x'`): no bindings,
                    // but still a cross-file edge — retain the source in
                    // the bindingless inventory.
                    result
                        .extracted
                        .bindingless_import_sources
                        .push(import_decl.source.value.to_string());
                    continue;
                };
                if specifiers.is_empty() {
                    // `import {} from './x'`: binds nothing, still an edge.
                    result
                        .extracted
                        .bindingless_import_sources
                        .push(import_decl.source.value.to_string());
                    continue;
                }
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
                        with_bodies,
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
            // `export = X` — a CommonJS-style export assignment. Capture the
            // assigned local VALUE name so `typeof import("./m")` can resolve
            // the whole module to `typeof X`. Only a bare identifier target is
            // captured (the `export = SomeValue` form); a non-identifier
            // expression has no addressable value root and is left `None`.
            Statement::TSExportAssignment(assignment) => {
                if let Expression::Identifier(ident) = &assignment.expression {
                    result.export_assignment_target = Some(ident.name.to_string());
                }
            }
            Statement::TSTypeAliasDeclaration(type_alias) => {
                let deps = if with_bodies {
                    type_alias_dependency_names(type_alias)
                } else {
                    DeclDependencyNames::default()
                };
                result.local_type_symbols.insert(
                    type_alias.id.name.to_string(),
                    AnalyzedExternalTypeSymbol {
                        kind: AnalyzedExternalTypeSymbolKind::TypeAlias,
                        span: type_alias.span.into(),
                        dependency_names: deps.dependency_names,
                        structural_dependency_names: deps.structural_dependency_names,
                    },
                );
            }
            Statement::TSInterfaceDeclaration(interface) => {
                let deps = if with_bodies {
                    interface_dependency_names(interface)
                } else {
                    DeclDependencyNames::default()
                };
                result.local_type_symbols.insert(
                    interface.id.name.to_string(),
                    AnalyzedExternalTypeSymbol {
                        kind: AnalyzedExternalTypeSymbolKind::Interface,
                        span: interface.span.into(),
                        dependency_names: deps.dependency_names,
                        structural_dependency_names: deps.structural_dependency_names,
                    },
                );
            }
            Statement::ClassDeclaration(class_decl) => {
                if let Some(id) = &class_decl.id {
                    let deps = if with_bodies {
                        class_dependency_names(class_decl)
                    } else {
                        DeclDependencyNames::default()
                    };
                    result.local_type_symbols.insert(
                        id.name.to_string(),
                        AnalyzedExternalTypeSymbol {
                            kind: AnalyzedExternalTypeSymbolKind::Class,
                            span: class_decl.span.into(),
                            dependency_names: deps.dependency_names,
                            structural_dependency_names: deps.structural_dependency_names,
                        },
                    );
                }
            }
            Statement::ExportDefaultDeclaration(export_default) => {
                match &export_default.declaration {
                    ExportDefaultDeclarationKind::ClassDeclaration(class_decl) => {
                        let deps = if with_bodies {
                            class_dependency_names(class_decl)
                        } else {
                            DeclDependencyNames::default()
                        };
                        result.local_type_symbols.insert(
                            "default".to_string(),
                            AnalyzedExternalTypeSymbol {
                                kind: AnalyzedExternalTypeSymbolKind::Class,
                                span: export_default.span.into(),
                                dependency_names: deps.dependency_names,
                                structural_dependency_names: deps.structural_dependency_names,
                            },
                        );
                    }
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                        let deps = if with_bodies {
                            interface_dependency_names(interface)
                        } else {
                            DeclDependencyNames::default()
                        };
                        result.local_type_symbols.insert(
                            "default".to_string(),
                            AnalyzedExternalTypeSymbol {
                                kind: AnalyzedExternalTypeSymbolKind::Interface,
                                span: export_default.span.into(),
                                dependency_names: deps.dependency_names,
                                structural_dependency_names: deps.structural_dependency_names,
                            },
                        );
                    }
                    // `export default <ident>` (e.g. `export default leafDefault`)
                    // — the default export's VALUE is the referenced local
                    // binding. Map the `default` export name to that local value
                    // name so `typeof import("./m").default` resolves to
                    // `typeof <ident>` (the export-target chase reaches the
                    // local value decl).
                    ExportDefaultDeclarationKind::Identifier(ident) => {
                        result
                            .local_export_symbol_targets
                            .insert("default".to_string(), ident.name.to_string());
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Oracle harness: capture the parse-time `RawSourceSurface` raw-fact
    // inventory while the OXC arena is still live. BODY data — captured
    // only on the with-bodies path; the header-only artifact analyzer
    // leaves the inventory empty (per-symbol raw surfaces are demand
    // products of the lazy declaration-body memo).
    if with_bodies {
        let captured = merge_overload_groups(
            program
                .body
                .iter()
                .flat_map(capture_statement_surfaces)
                .collect(),
        );
        for c in captured {
            // Append in source order: a MERGED declaration shares one
            // `(name, space)` triple across several contributors, so each is
            // RETAINED (not last-wins overwritten) for the source-side
            // walk's per-contributor allowlist check.
            result
                .raw_source_surfaces
                .entry((c.name, c.symbol_space))
                .or_default()
                .push(c.surface);
        }
    }

    result
}

fn record_local_type_symbol_from_declaration(
    declaration: &Declaration<'_>,
    local_type_symbols: &mut FxHashMap<String, AnalyzedExternalTypeSymbol>,
    with_bodies: bool,
) {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => {
            let deps = if with_bodies {
                type_alias_dependency_names(type_alias)
            } else {
                DeclDependencyNames::default()
            };
            local_type_symbols.insert(
                type_alias.id.name.to_string(),
                AnalyzedExternalTypeSymbol {
                    kind: AnalyzedExternalTypeSymbolKind::TypeAlias,
                    span: type_alias.span.into(),
                    dependency_names: deps.dependency_names,
                    structural_dependency_names: deps.structural_dependency_names,
                },
            );
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            let deps = if with_bodies {
                interface_dependency_names(interface)
            } else {
                DeclDependencyNames::default()
            };
            local_type_symbols.insert(
                interface.id.name.to_string(),
                AnalyzedExternalTypeSymbol {
                    kind: AnalyzedExternalTypeSymbolKind::Interface,
                    span: interface.span.into(),
                    dependency_names: deps.dependency_names,
                    structural_dependency_names: deps.structural_dependency_names,
                },
            );
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                let deps = if with_bodies {
                    class_dependency_names(class_decl)
                } else {
                    DeclDependencyNames::default()
                };
                local_type_symbols.insert(
                    id.name.to_string(),
                    AnalyzedExternalTypeSymbol {
                        kind: AnalyzedExternalTypeSymbolKind::Class,
                        span: class_decl.span.into(),
                        dependency_names: deps.dependency_names,
                        structural_dependency_names: deps.structural_dependency_names,
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

/// The reference-name pair one declaration's BODY contributes: the plain
/// dependency names plus the structural subset. This is the per-statement
/// demand product the lazy declaration-body path consumes — computed for
/// exactly the demanded declaration, never for every symbol in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclDependencyNames {
    pub dependency_names: FxHashSet<String>,
    pub structural_dependency_names: FxHashSet<String>,
}

/// Collect the `(declared name, body reference names)` pairs ONE top-level
/// statement contributes, keyed under exactly the env-symbol names the
/// LOWERER (`lower_top_level_statement`) registers — the names the lazy
/// declaration-body memo iterates. This is NOT the analyzer's keying: the
/// default-exported class / interface arms emit deps under BOTH the
/// declared name AND the `default` alias (mirroring
/// `alias_default_export_type_symbol`), whereas the analyzer keys a
/// default export under `"default"` only. Covers type aliases, interfaces,
/// named classes, their `export` wrappers, identifier-namespace inner
/// declarations (under qualified `Ns.Name` keys), and the default export.
/// A statement that declares no type-space symbol yields an empty vector.
pub fn collect_statement_dependency_names(
    stmt: &Statement<'_>,
) -> Vec<(String, DeclDependencyNames)> {
    match stmt {
        Statement::TSTypeAliasDeclaration(type_alias) => vec![(
            type_alias.id.name.to_string(),
            type_alias_dependency_names(type_alias),
        )],
        Statement::TSInterfaceDeclaration(interface) => vec![(
            interface.id.name.to_string(),
            interface_dependency_names(interface),
        )],
        Statement::ClassDeclaration(class_decl) => class_decl
            .id
            .as_ref()
            .map(|id| vec![(id.name.to_string(), class_dependency_names(class_decl))])
            .unwrap_or_default(),
        Statement::TSModuleDeclaration(module) => {
            // An identifier namespace registers its inner type declarations
            // under qualified `Ns.Name` keys (matching `lower_top_level_
            // statement`'s `extract_module_declaration`); a string-literal
            // ambient module is an augmentation scope whose dep edges ride
            // on the per-contributor `FileWholeHash` rail — nothing to
            // collect here.
            let mut out = Vec::new();
            collect_module_dependency_names(module, None, &mut out);
            out
        }
        Statement::ExportNamedDeclaration(export) => export
            .declaration
            .as_ref()
            .map(collect_declaration_dependency_names)
            .unwrap_or_default(),
        Statement::ExportDefaultDeclaration(export_default) => match &export_default.declaration {
            // The default class lowers under BOTH its declared name and the
            // `default` alias (see `alias_default_export_type_symbol`), so
            // emit the heritage deps under both keys — an anonymous default
            // class has no declared name, only `default`.
            ExportDefaultDeclarationKind::ClassDeclaration(class_decl) => {
                let deps = class_dependency_names(class_decl);
                match &class_decl.id {
                    Some(id) => vec![
                        (id.name.to_string(), deps.clone()),
                        ("default".to_string(), deps),
                    ],
                    None => vec![("default".to_string(), deps)],
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                let deps = interface_dependency_names(interface);
                vec![
                    (interface.id.name.to_string(), deps.clone()),
                    ("default".to_string(), deps),
                ]
            }
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Collect `(qualified name, body reference names)` pairs for an
/// identifier namespace's inner type declarations, mirroring
/// `extract_module_declaration` / `extract_namespaced_statement` so the
/// dep-record keys match the `Ns.Name` keys the env walk lowers under.
/// A string-literal ambient module (augmentation scope) contributes
/// nothing here.
fn collect_module_dependency_names(
    module: &TSModuleDeclaration<'_>,
    prefix: Option<&str>,
    out: &mut Vec<(String, DeclDependencyNames)>,
) {
    let namespace = match &module.id {
        TSModuleDeclarationName::Identifier(id) => match prefix {
            Some(prefix) => format!("{prefix}.{}", id.name),
            None => id.name.to_string(),
        },
        TSModuleDeclarationName::StringLiteral(_) => return,
    };
    let Some(body) = module.body.as_ref() else {
        return;
    };
    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
            collect_module_dependency_names(inner, Some(namespace.as_str()), out);
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for stmt in &block.body {
                collect_namespaced_statement_dependency_names(stmt, namespace.as_str(), out);
            }
        }
    }
}

fn collect_namespaced_statement_dependency_names(
    stmt: &Statement<'_>,
    namespace: &str,
    out: &mut Vec<(String, DeclDependencyNames)>,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            out.push((
                format!("{namespace}.{}", alias.id.name),
                type_alias_dependency_names(alias),
            ));
        }
        Statement::TSInterfaceDeclaration(interface) => {
            out.push((
                format!("{namespace}.{}", interface.id.name),
                interface_dependency_names(interface),
            ));
        }
        Statement::TSModuleDeclaration(module) => {
            collect_module_dependency_names(module, Some(namespace), out);
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = export.declaration.as_ref() {
                collect_namespaced_declaration_dependency_names(decl, namespace, out);
            }
        }
        _ => {}
    }
}

fn collect_namespaced_declaration_dependency_names(
    decl: &Declaration<'_>,
    namespace: &str,
    out: &mut Vec<(String, DeclDependencyNames)>,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            out.push((
                format!("{namespace}.{}", alias.id.name),
                type_alias_dependency_names(alias),
            ));
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            out.push((
                format!("{namespace}.{}", interface.id.name),
                interface_dependency_names(interface),
            ));
        }
        Declaration::TSModuleDeclaration(module) => {
            collect_module_dependency_names(module, Some(namespace), out);
        }
        _ => {}
    }
}

fn collect_declaration_dependency_names(
    declaration: &Declaration<'_>,
) -> Vec<(String, DeclDependencyNames)> {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => vec![(
            type_alias.id.name.to_string(),
            type_alias_dependency_names(type_alias),
        )],
        Declaration::TSInterfaceDeclaration(interface) => vec![(
            interface.id.name.to_string(),
            interface_dependency_names(interface),
        )],
        Declaration::ClassDeclaration(class_decl) => class_decl
            .id
            .as_ref()
            .map(|id| vec![(id.name.to_string(), class_dependency_names(class_decl))])
            .unwrap_or_default(),
        Declaration::TSModuleDeclaration(module) => {
            // `export namespace N { … }` — collect its inner type
            // declarations under qualified `N.Name` keys.
            let mut out = Vec::new();
            collect_module_dependency_names(module, None, &mut out);
            out
        }
        _ => Vec::new(),
    }
}

fn type_alias_dependency_names(type_alias: &TSTypeAliasDeclaration<'_>) -> DeclDependencyNames {
    let mut out = DeclDependencyNames::default();
    collect_type_reference_names(&type_alias.type_annotation, &mut out.dependency_names);
    collect_structural_type_reference_names(
        &type_alias.type_annotation,
        StructuralDependencyContext::Root,
        &mut out.structural_dependency_names,
    );
    out
}

fn interface_dependency_names(interface: &TSInterfaceDeclaration<'_>) -> DeclDependencyNames {
    let mut out = DeclDependencyNames::default();
    for parent in &interface.extends {
        if let Some(name) = get_expression_reference_name(&parent.expression) {
            out.dependency_names.insert(name.clone());
            out.structural_dependency_names.insert(name);
        }
        if let Some(type_arguments) = &parent.type_arguments {
            for param in &type_arguments.params {
                collect_type_reference_names(param, &mut out.dependency_names);
                collect_structural_type_reference_names(
                    param,
                    StructuralDependencyContext::Root,
                    &mut out.structural_dependency_names,
                );
            }
        }
    }
    collect_interface_reference_names(
        &interface.body.body,
        &interface.extends,
        &mut out.dependency_names,
    );
    collect_structural_interface_reference_names(
        &interface.body.body,
        &interface.extends,
        &mut out.structural_dependency_names,
    );
    out
}

fn class_dependency_names(class_decl: &Class<'_>) -> DeclDependencyNames {
    let mut out = DeclDependencyNames::default();
    collect_class_reference_names(class_decl, &mut out.dependency_names);
    collect_structural_class_reference_names(class_decl, &mut out.structural_dependency_names);
    out
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
fn get_expression_reference_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn get_type_reference_name(type_name: &TSTypeName<'_>) -> String {
    match type_name {
        TSTypeName::IdentifierReference(id) => id.name.to_string(),
        TSTypeName::QualifiedName(qualified) => {
            let left = get_type_reference_name(&qualified.left);
            format!("{}.{}", left, qualified.right.name)
        }
        TSTypeName::ThisExpression(_) => "this".to_string(),
    }
}
