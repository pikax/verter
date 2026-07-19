//! Framework-neutral script routing and declaration inventory.
//!
//! Captures imports, exports, local declaration headers, and structural
//! dependency names from one parsed program. It performs no type resolution
//! and produces no compiler-facing semantic surface.

use std::collections::{BTreeMap, BTreeSet};
use std::str;
use std::sync::Arc;

use oxc_ast::ast::*;
use verter_type_expr::facts::TypeDependencyPathFact;
use verter_type_expr::{DeclKey, TopLevelOwnerId};

use crate::common::Span;
use crate::utils::oxc::script::raw_surface::{
    capture_statement_surfaces, merge_overload_groups, CapturedSurface, RawSourceSurface,
    SymbolSpace,
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum SyntaxCapability {
    TypeOnly,
    ValueOnly,
    TypeAndValue,
}

impl SyntaxCapability {
    fn from_import_kind(kind: ImportOrExportKind) -> Self {
        match kind {
            ImportOrExportKind::Type => Self::TypeOnly,
            ImportOrExportKind::Value => Self::TypeAndValue,
        }
    }

    fn from_export_kind(kind: ImportOrExportKind) -> Self {
        match kind {
            ImportOrExportKind::Type => Self::TypeOnly,
            ImportOrExportKind::Value => Self::TypeAndValue,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ImportBindingForm {
    Named,
    Default,
    Namespace,
}

/// Owner-qualified declaration path. `root` is the lexical binding; namespace
/// membership remains an ordered structural path and is never dotted text.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct DeclarationPath {
    pub root: DeclKey,
    members: Arc<[String]>,
}

impl<'de> serde::Deserialize<'de> for DeclarationPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct DeclarationPathWire {
            root: DeclKey,
            members: Vec<String>,
        }

        let wire = <DeclarationPathWire as serde::Deserialize>::deserialize(deserializer)?;
        if wire.root.name.trim().is_empty() {
            return Err(serde::de::Error::custom(
                "declaration root name must not be empty or whitespace-only",
            ));
        }
        if wire.members.iter().any(|member| member.trim().is_empty()) {
            return Err(serde::de::Error::custom(
                "declaration member path must not contain empty or whitespace-only segments",
            ));
        }
        Ok(Self {
            root: wire.root,
            members: Arc::from(wire.members.into_boxed_slice()),
        })
    }
}

impl DeclarationPath {
    #[must_use]
    pub fn new<I, S>(root: DeclKey, members: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            root,
            members: Arc::from(
                members
                    .into_iter()
                    .map(Into::into)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
        }
    }

    #[must_use]
    pub fn root(root: DeclKey) -> Self {
        Self::new(root, std::iter::empty::<String>())
    }

    #[must_use]
    pub fn from_dependency(owner: TopLevelOwnerId, path: &TypeDependencyPathFact) -> Self {
        Self::new(
            DeclKey::new(owner, path.root()),
            path.member_path().iter().cloned(),
        )
    }

    #[must_use]
    pub fn member_path(&self) -> &[String] {
        &self.members
    }

    #[must_use]
    pub fn appended(&self, member: impl Into<String>) -> Self {
        let mut members = self.members.to_vec();
        members.push(member.into());
        Self::new(self.root.clone(), members)
    }

    fn namespace_prefix(&self) -> Self {
        let prefix_len = self.members.len().saturating_sub(1);
        Self::new(
            self.root.clone(),
            self.members[..prefix_len].iter().cloned(),
        )
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ImportedExportPath {
    NamespaceRoot,
    Symbol(TypeDependencyPathFact),
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct RoutedImportTarget {
    pub source: String,
    pub form: ImportBindingForm,
    pub capability: SyntaxCapability,
    pub exported: ImportedExportPath,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ImportedTypeBinding {
    pub local: DeclKey,
    pub source: String,
    pub form: ImportBindingForm,
    pub capability: SyntaxCapability,
    /// Named/default imports have a structural target root. Namespace imports
    /// acquire their exported symbol only from the dependency path at routing.
    pub imported: ImportedExportPath,
}

impl ImportedTypeBinding {
    fn route(&self, dependency: &TypeDependencyPathFact) -> Option<RoutedImportTarget> {
        if dependency.root() != self.local.name.as_ref() {
            return None;
        }
        let exported = match (&self.imported, dependency.member_path()) {
            (ImportedExportPath::NamespaceRoot, []) => ImportedExportPath::NamespaceRoot,
            (ImportedExportPath::NamespaceRoot, members) => ImportedExportPath::Symbol(
                TypeDependencyPathFact::from_segments(members.iter().cloned())?,
            ),
            (ImportedExportPath::Symbol(base), members) => {
                let mut segments = base.segments().to_vec();
                segments.extend(members.iter().cloned());
                ImportedExportPath::Symbol(TypeDependencyPathFact::from_segments(segments)?)
            }
        };
        Some(RoutedImportTarget {
            source: self.source.clone(),
            form: self.form,
            capability: self.capability,
            exported,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ReexportBinding {
    pub exported: DeclKey,
    pub target: ExportTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BindinglessImport {
    pub owner: TopLevelOwnerId,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WildcardReexport {
    pub owner: TopLevelOwnerId,
    pub source: String,
    pub capability: SyntaxCapability,
    pub exported_namespace: Option<DeclKey>,
}

/// Result of extracting type bindings from a dependency file.
/// Includes named bindings (from `import` and `export {} from`),
/// wildcard re-export sources (from `export * from`), and bindingless
/// import sources (from `import './x'` / `import {} from './x'`).
#[derive(Debug, Clone, Default)]
pub struct ExtractedTypeBindings {
    pub bindings: Vec<ImportedTypeBinding>,
    pub reexport_bindings: Vec<ReexportBinding>,
    pub wildcard_reexports: Vec<WildcardReexport>,
    /// Import declarations that bind NO local name — side-effect imports
    /// (`import './x'`) and empty named-import lists (`import {} from
    /// './x'`). They still create a cross-file dependency edge (the
    /// specifier resolves to a canonical file), so the shallow edge
    /// inventory must retain them: edge-currency oracles treat any
    /// cross-file edge as dependency-set-derived state that can go stale
    /// when the file set moves. In declaration order.
    pub bindingless_imports: Vec<BindinglessImport>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum ExportTarget {
    LocalDeclaration {
        declaration: DeclarationPath,
        capability: SyntaxCapability,
    },
    LocalBinding {
        binding: DeclKey,
        capability: SyntaxCapability,
    },
    External(RoutedImportTarget),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UniqueResolution<T> {
    Unique(T),
    Ambiguous,
}

fn insert_unique<K, V>(map: &mut BTreeMap<K, UniqueResolution<V>>, key: K, value: V)
where
    K: Ord,
    V: PartialEq,
{
    match map.entry(key) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(UniqueResolution::Unique(value));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if !matches!(entry.get(), UniqueResolution::Unique(existing) if existing == &value) {
                entry.insert(UniqueResolution::Ambiguous);
            }
        }
    }
}

fn insert_strict<K, V>(map: &mut BTreeMap<K, UniqueResolution<V>>, key: K, value: V)
where
    K: Ord,
{
    match map.entry(key) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(UniqueResolution::Unique(value));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            entry.insert(UniqueResolution::Ambiguous);
        }
    }
}

fn insert_local_declaration_export(
    exports: &mut BTreeMap<DeclKey, UniqueResolution<ExportTarget>>,
    exported: DeclKey,
    declaration: DeclarationPath,
    capability: SyntaxCapability,
) {
    match exports.entry(exported) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(UniqueResolution::Unique(ExportTarget::LocalDeclaration {
                declaration,
                capability,
            }));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => match entry.get_mut() {
            UniqueResolution::Unique(ExportTarget::LocalDeclaration {
                declaration: existing_declaration,
                capability: existing_capability,
            }) if existing_declaration == &declaration => {
                if capability == SyntaxCapability::TypeAndValue {
                    *existing_capability = capability;
                }
            }
            UniqueResolution::Unique(_) | UniqueResolution::Ambiguous => {
                entry.insert(UniqueResolution::Ambiguous);
            }
        },
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum AnalyzedExternalTypeSymbolKind {
    TypeAlias,
    Interface,
    Class,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TypeDeclarationContributor {
    pub kind: AnalyzedExternalTypeSymbolKind,
    pub span: Span,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum TypeDeclarationMergePolicy {
    Single,
    Interface,
    ClassInterface,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AnalyzedExternalTypeSymbol {
    /// Canonical effective kind. Callers should obtain the symbol through
    /// `AnalyzedExternalTypeSource::local_type_symbol`, which rejects invalid
    /// declaration merges.
    pub kind: AnalyzedExternalTypeSymbolKind,
    /// Canonical contributor span: the class contributor for class/interface
    /// merges, otherwise the first authored contributor.
    pub span: Span,
    pub contributors: Vec<TypeDeclarationContributor>,
    pub merge_policy: TypeDeclarationMergePolicy,
    pub dependency_paths: BTreeSet<TypeDependencyPathFact>,
    pub structural_dependency_paths: BTreeSet<TypeDependencyPathFact>,
    pub declaration_carrier_paths: BTreeSet<TypeDependencyPathFact>,
    pub value_query_paths: BTreeSet<TypeDependencyPathFact>,
    pub value_position_paths: BTreeSet<TypeDependencyPathFact>,
    pub unsupported_value_positions: BTreeSet<UnsupportedValuePositionKind>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum UnsupportedValuePositionKind {
    ClassHeritageExpression,
    ComputedSignatureKey,
    ComputedClassKey,
}

/// Authored top-level namespace statement that carries one qualified local
/// type symbol. Multiple rows preserve namespace-merge contributors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceTypeCarrier {
    pub declaration: DeclarationPath,
    pub carrier: DeclKey,
    pub carrier_span: Span,
}

impl AnalyzedExternalTypeSymbol {
    fn from_dependencies(
        kind: AnalyzedExternalTypeSymbolKind,
        span: Span,
        dependencies: DeclDependencyNames,
    ) -> Self {
        Self {
            kind,
            span,
            contributors: vec![TypeDeclarationContributor { kind, span }],
            merge_policy: TypeDeclarationMergePolicy::Single,
            dependency_paths: dependencies.dependency_paths,
            structural_dependency_paths: dependencies.structural_dependency_paths,
            declaration_carrier_paths: dependencies.declaration_carrier_paths,
            value_query_paths: dependencies.value_query_paths,
            value_position_paths: dependencies.value_position_paths,
            unsupported_value_positions: dependencies.unsupported_value_positions,
        }
    }

    fn merge_dependencies(&mut self, other: Self) {
        self.contributors.extend(other.contributors);
        self.recompute_merge_policy();
        self.dependency_paths.extend(other.dependency_paths);
        self.structural_dependency_paths
            .extend(other.structural_dependency_paths);
        self.declaration_carrier_paths
            .extend(other.declaration_carrier_paths);
        self.value_query_paths.extend(other.value_query_paths);
        self.value_position_paths.extend(other.value_position_paths);
        self.unsupported_value_positions
            .extend(other.unsupported_value_positions);
    }

    fn recompute_merge_policy(&mut self) {
        let alias_count = self
            .contributors
            .iter()
            .filter(|contributor| contributor.kind == AnalyzedExternalTypeSymbolKind::TypeAlias)
            .count();
        let interface_count = self
            .contributors
            .iter()
            .filter(|contributor| contributor.kind == AnalyzedExternalTypeSymbolKind::Interface)
            .count();
        let class_count = self
            .contributors
            .iter()
            .filter(|contributor| contributor.kind == AnalyzedExternalTypeSymbolKind::Class)
            .count();

        self.merge_policy = match (alias_count, interface_count, class_count) {
            (1, 0, 0) | (0, 1, 0) | (0, 0, 1) => TypeDeclarationMergePolicy::Single,
            (0, 2.., 0) => TypeDeclarationMergePolicy::Interface,
            (0, 1.., 1) => TypeDeclarationMergePolicy::ClassInterface,
            _ => TypeDeclarationMergePolicy::Invalid,
        };

        let primary = if self.merge_policy == TypeDeclarationMergePolicy::ClassInterface {
            self.contributors
                .iter()
                .find(|contributor| contributor.kind == AnalyzedExternalTypeSymbolKind::Class)
        } else {
            self.contributors.first()
        };
        if let Some(primary) = primary {
            self.kind = primary.kind;
            self.span = primary.span;
        }
    }

    #[must_use]
    pub fn primary_kind(&self) -> Option<AnalyzedExternalTypeSymbolKind> {
        (self.merge_policy != TypeDeclarationMergePolicy::Invalid).then_some(self.kind)
    }

    fn capability(&self) -> Option<SyntaxCapability> {
        match self.merge_policy {
            TypeDeclarationMergePolicy::Invalid => None,
            TypeDeclarationMergePolicy::ClassInterface => Some(SyntaxCapability::TypeAndValue),
            TypeDeclarationMergePolicy::Single
                if self.kind == AnalyzedExternalTypeSymbolKind::Class =>
            {
                Some(SyntaxCapability::TypeAndValue)
            }
            TypeDeclarationMergePolicy::Single | TypeDeclarationMergePolicy::Interface => {
                Some(SyntaxCapability::TypeOnly)
            }
        }
    }
}

fn insert_or_merge_type_symbol(
    symbols: &mut BTreeMap<DeclarationPath, AnalyzedExternalTypeSymbol>,
    declaration: DeclarationPath,
    symbol: AnalyzedExternalTypeSymbol,
) {
    match symbols.entry(declaration) {
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            entry.get_mut().merge_dependencies(symbol);
        }
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(symbol);
        }
    }
}

fn restrict_export_capability(
    authored: SyntaxCapability,
    available: SyntaxCapability,
) -> Option<SyntaxCapability> {
    match (authored, available) {
        (
            SyntaxCapability::TypeOnly,
            SyntaxCapability::TypeOnly | SyntaxCapability::TypeAndValue,
        ) => Some(SyntaxCapability::TypeOnly),
        (
            SyntaxCapability::ValueOnly,
            SyntaxCapability::ValueOnly | SyntaxCapability::TypeAndValue,
        ) => Some(SyntaxCapability::ValueOnly),
        (SyntaxCapability::TypeAndValue, capability) => Some(capability),
        (SyntaxCapability::TypeOnly, SyntaxCapability::ValueOnly)
        | (SyntaxCapability::ValueOnly, SyntaxCapability::TypeOnly) => None,
    }
}

fn reconcile_export_targets(source: &mut AnalyzedExternalTypeSource) {
    let exports = std::mem::take(&mut source.exports);
    source.exports = exports
        .into_iter()
        .map(|(exported, resolution)| {
            let resolution = match resolution {
                UniqueResolution::Ambiguous => UniqueResolution::Ambiguous,
                UniqueResolution::Unique(ExportTarget::LocalDeclaration {
                    declaration,
                    capability,
                }) => match source.local_type_symbols.get(&declaration) {
                    Some(symbol) => match symbol.capability() {
                        Some(available) => restrict_export_capability(capability, available)
                            .map_or(UniqueResolution::Ambiguous, |capability| {
                                UniqueResolution::Unique(ExportTarget::LocalDeclaration {
                                    declaration,
                                    capability,
                                })
                            }),
                        None => UniqueResolution::Ambiguous,
                    },
                    None => UniqueResolution::Unique(ExportTarget::LocalDeclaration {
                        declaration,
                        capability,
                    }),
                },
                UniqueResolution::Unique(ExportTarget::LocalBinding {
                    binding,
                    capability,
                }) => {
                    let local_declaration = DeclarationPath::root(binding.clone());
                    let available = source
                        .local_type_symbols
                        .get(&local_declaration)
                        .map(AnalyzedExternalTypeSymbol::capability)
                        .or_else(|| {
                            source
                                .imports
                                .get(&binding)
                                .map(|resolution| match resolution {
                                    UniqueResolution::Unique(binding) => Some(binding.capability),
                                    UniqueResolution::Ambiguous => None,
                                })
                        });
                    match available {
                        Some(Some(available)) => restrict_export_capability(capability, available)
                            .map_or(UniqueResolution::Ambiguous, |capability| {
                                UniqueResolution::Unique(ExportTarget::LocalBinding {
                                    binding,
                                    capability,
                                })
                            }),
                        Some(None) => UniqueResolution::Ambiguous,
                        None => UniqueResolution::Unique(ExportTarget::LocalBinding {
                            binding,
                            capability,
                        }),
                    }
                }
                UniqueResolution::Unique(target @ ExportTarget::External(_)) => {
                    UniqueResolution::Unique(target)
                }
            };
            (exported, resolution)
        })
        .collect();
}

#[derive(Debug, Clone, Default)]
pub struct AnalyzedExternalTypeSource {
    pub extracted: ExtractedTypeBindings,
    imports: BTreeMap<DeclKey, UniqueResolution<ImportedTypeBinding>>,
    exports: BTreeMap<DeclKey, UniqueResolution<ExportTarget>>,
    exported_local_type_declarations: BTreeSet<DeclarationPath>,
    local_type_symbols: BTreeMap<DeclarationPath, AnalyzedExternalTypeSymbol>,
    namespace_type_carriers: BTreeMap<DeclarationPath, Vec<NamespaceTypeCarrier>>,
    top_level_statement_count: usize,
    /// Parse-time `RawSourceSurface` raw-fact inventory (oracle harness design
    /// item G), keyed by `(name, symbol_space)` within this file. Captured while
    /// the OXC arena is live, before lowering erases the §Q2 facts.
    raw_source_surfaces: BTreeMap<(DeclarationPath, SymbolSpace), Vec<RawSourceSurface>>,
    /// The local value name a CommonJS-style `export = X` assigns the whole
    /// module to, when present. `typeof import("./m")` against such a module
    /// resolves to `typeof X` (the export-assignment value), not an object
    /// wrapping the named exports. `None` for an ordinary ESM module.
    export_assignment_targets: BTreeMap<TopLevelOwnerId, UniqueResolution<DeclKey>>,
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
    pub fn required_import_bindings(&self, declaration: &DeclarationPath) -> BTreeSet<DeclKey> {
        let mut required = BTreeSet::new();
        let mut visited = BTreeSet::new();
        let mut pending = vec![declaration.clone()];
        while let Some(current) = pending.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            if self.import_binding(&current.root).is_some() {
                required.insert(current.root);
                continue;
            }
            let Some(symbol) = self.local_type_symbol(&current) else {
                continue;
            };
            for dependency in &symbol.structural_dependency_paths {
                if let Some(local) = self.resolve_local_dependency(&current, dependency) {
                    if !visited.contains(&local) {
                        pending.push(local);
                    }
                } else {
                    let import = DeclKey::new(current.root.owner, dependency.root());
                    if self.import_binding(&import).is_some() {
                        required.insert(import);
                    }
                }
            }
        }
        required
    }

    pub fn import_binding(&self, local: &DeclKey) -> Option<&ImportedTypeBinding> {
        match self.imports.get(local) {
            Some(UniqueResolution::Unique(binding)) => Some(binding),
            Some(UniqueResolution::Ambiguous) | None => None,
        }
    }

    pub fn is_ambiguous_import(&self, local: &DeclKey) -> bool {
        matches!(self.imports.get(local), Some(UniqueResolution::Ambiguous))
    }

    pub fn resolve_import_dependency(
        &self,
        owner: TopLevelOwnerId,
        dependency: &TypeDependencyPathFact,
    ) -> Option<RoutedImportTarget> {
        self.import_binding(&DeclKey::new(owner, dependency.root()))?
            .route(dependency)
    }

    pub fn export_target(&self, exported: &DeclKey) -> Option<&ExportTarget> {
        match self.exports.get(exported) {
            Some(UniqueResolution::Unique(target)) => Some(target),
            Some(UniqueResolution::Ambiguous) | None => None,
        }
    }

    pub fn is_ambiguous_export(&self, exported: &DeclKey) -> bool {
        matches!(
            self.exports.get(exported),
            Some(UniqueResolution::Ambiguous)
        )
    }

    pub fn direct_reexport_target(&self, exported: &DeclKey) -> Option<&RoutedImportTarget> {
        match self.export_target(exported)? {
            ExportTarget::External(target) => Some(target),
            ExportTarget::LocalDeclaration { .. } | ExportTarget::LocalBinding { .. } => None,
        }
    }

    pub fn local_import_symbol_target(&self, local: &DeclKey) -> Option<&ImportedTypeBinding> {
        self.import_binding(local)
    }

    pub fn local_export_symbol_target(&self, exported: &DeclKey) -> Option<&ExportTarget> {
        self.export_target(exported)
    }

    /// The local value name a CommonJS `export = X` assigns the whole module
    /// to (`Some("X")`), or `None` for an ordinary ESM module.
    pub fn export_assignment_target(&self, owner: TopLevelOwnerId) -> Option<&DeclKey> {
        match self.export_assignment_targets.get(&owner) {
            Some(UniqueResolution::Unique(target)) => Some(target),
            Some(UniqueResolution::Ambiguous) | None => None,
        }
    }

    pub fn is_ambiguous_export_assignment(&self, owner: TopLevelOwnerId) -> bool {
        matches!(
            self.export_assignment_targets.get(&owner),
            Some(UniqueResolution::Ambiguous)
        )
    }

    pub fn exported_local_type_declarations(&self) -> impl Iterator<Item = &DeclarationPath> {
        self.exported_local_type_declarations.iter()
    }

    pub fn exported_symbol_keys(&self) -> impl Iterator<Item = &DeclKey> {
        self.exports.keys()
    }

    pub fn direct_reexport_entries(&self) -> impl Iterator<Item = (&DeclKey, &RoutedImportTarget)> {
        self.exports.iter().filter_map(|(exported, resolution)| {
            let UniqueResolution::Unique(ExportTarget::External(target)) = resolution else {
                return None;
            };
            Some((exported, target))
        })
    }

    pub fn wildcard_reexports(&self) -> &[WildcardReexport] {
        &self.extracted.wildcard_reexports
    }

    pub fn local_symbol_span(&self, declaration: &DeclarationPath) -> Option<Span> {
        self.local_type_symbol(declaration)
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
    pub fn local_type_symbol_spans(&self) -> impl Iterator<Item = (&DeclarationPath, Span)> {
        self.local_type_symbols
            .iter()
            .filter_map(|(declaration, symbol)| {
                (symbol.merge_policy != TypeDeclarationMergePolicy::Invalid)
                    .then_some((declaration, symbol.span))
            })
    }

    pub fn local_type_symbol(
        &self,
        declaration: &DeclarationPath,
    ) -> Option<&AnalyzedExternalTypeSymbol> {
        self.local_type_symbols
            .get(declaration)
            .filter(|symbol| symbol.merge_policy != TypeDeclarationMergePolicy::Invalid)
    }

    pub fn is_ambiguous_local_type_symbol(&self, declaration: &DeclarationPath) -> bool {
        self.local_type_symbols
            .get(declaration)
            .is_some_and(|symbol| symbol.merge_policy == TypeDeclarationMergePolicy::Invalid)
    }

    pub fn namespace_type_carriers(
        &self,
        declaration: &DeclarationPath,
    ) -> &[NamespaceTypeCarrier] {
        self.namespace_type_carriers
            .get(declaration)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn local_symbol_target(&self, exported: &DeclKey) -> Option<&DeclarationPath> {
        match self.export_target(exported)? {
            ExportTarget::LocalDeclaration { declaration, .. } => Some(declaration),
            ExportTarget::LocalBinding { .. } | ExportTarget::External(_) => None,
        }
    }

    pub fn has_local_symbol_target(&self, exported: &DeclKey) -> bool {
        self.local_symbol_target(exported)
            .is_some_and(|target| self.local_type_symbol(target).is_some())
    }

    pub fn local_symbol_dependency_paths(
        &self,
        declaration: &DeclarationPath,
    ) -> BTreeSet<DeclarationPath> {
        let mut dependencies = BTreeSet::new();
        let Some(symbol) = self.local_type_symbol(declaration) else {
            return dependencies;
        };
        for reference in &symbol.dependency_paths {
            if let Some(local) = self.resolve_local_dependency(declaration, reference) {
                if &local != declaration {
                    dependencies.insert(local);
                }
            }
        }
        dependencies
    }

    fn resolve_local_dependency(
        &self,
        declaration: &DeclarationPath,
        dependency: &TypeDependencyPathFact,
    ) -> Option<DeclarationPath> {
        if !declaration.member_path().is_empty() {
            let namespace = declaration.namespace_prefix();
            for prefix_len in (0..=namespace.member_path().len()).rev() {
                let mut candidate = DeclarationPath::new(
                    namespace.root.clone(),
                    namespace.member_path()[..prefix_len].iter().cloned(),
                );
                for segment in dependency.segments() {
                    candidate = candidate.appended(segment.clone());
                }
                if self.local_type_symbol(&candidate).is_some() {
                    return Some(candidate);
                }
            }
        }
        let candidate = DeclarationPath::from_dependency(declaration.root.owner, dependency);
        self.local_type_symbol(&candidate)
            .is_some()
            .then_some(candidate)
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
        declaration: &DeclarationPath,
        symbol_space: SymbolSpace,
    ) -> &[RawSourceSurface] {
        self.raw_source_surfaces
            .get(&(declaration.clone(), symbol_space))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// The FIRST captured contributor for one `(name, symbol_space)`, if any — a
    /// convenience over [`Self::raw_source_surfaces_for`] for the
    /// single-contributor common case. Reads the same ordered vector; callers
    /// that must see EVERY merged contributor use `raw_source_surfaces_for`.
    pub fn raw_source_surface(
        &self,
        declaration: &DeclarationPath,
        symbol_space: SymbolSpace,
    ) -> Option<&RawSourceSurface> {
        self.raw_source_surfaces
            .get(&(declaration.clone(), symbol_space))
            .and_then(|v| v.first())
    }

    /// Enumerate every captured `(name, symbol_space)` contributor, flattening
    /// the ordered per-triple vectors.
    pub fn raw_source_surfaces(
        &self,
    ) -> impl Iterator<Item = ((&DeclarationPath, SymbolSpace), &RawSourceSurface)> {
        self.raw_source_surfaces
            .iter()
            .flat_map(|((declaration, space), surfaces)| {
                surfaces
                    .iter()
                    .map(move |surface| ((declaration, *space), surface))
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
            wildcard_reexport_count: self.extracted.wildcard_reexports.len(),
            import_local_count: self.imports.len(),
            local_type_symbol_count: self.local_type_symbols.len(),
            local_export_symbol_count: self.exports.len(),
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn analyze_external_type_source(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> AnalyzedExternalTypeSource {
    analyze_external_type_source_for_owner(dep_source, allocator, TopLevelOwnerId::ordinary_file())
}

pub fn analyze_external_type_source_for_owner(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
    owner: TopLevelOwnerId,
) -> AnalyzedExternalTypeSource {
    let source_type = oxc_span::SourceType::ts();
    let parsed = oxc_parser::Parser::new(allocator, dep_source, source_type).parse();

    if parsed.panicked {
        return AnalyzedExternalTypeSource::default();
    }

    analyze_external_type_program_for_owner(&parsed.program, owner)
}

pub fn extract_imported_type_bindings(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> ExtractedTypeBindings {
    analyze_external_type_source(dep_source, allocator).extracted
}

pub fn extract_imported_type_bindings_for_owner(
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
    owner: TopLevelOwnerId,
) -> ExtractedTypeBindings {
    analyze_external_type_source_for_owner(dep_source, allocator, owner).extracted
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
    pub exported: BTreeSet<DeclKey>,
    /// Source specifiers from `export * from '...'` declarations.
    pub wildcard_reexports: Vec<WildcardReexport>,
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
    extract_export_surface_for_owner(source, allocator, TopLevelOwnerId::ordinary_file())
}

pub fn extract_export_surface_for_owner(
    source: &str,
    allocator: &oxc_allocator::Allocator,
    owner: TopLevelOwnerId,
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
                            .exported
                            .insert(DeclKey::new(owner, specifier.exported.name().to_string()));
                    }
                    continue;
                }

                // Local re-export without source: export { Foo } / export { Foo as Bar }
                if !export_decl.specifiers.is_empty() {
                    for specifier in &export_decl.specifiers {
                        result
                            .exported
                            .insert(DeclKey::new(owner, specifier.exported.name().to_string()));
                    }
                    continue;
                }

                // Exported declaration: export interface/type/enum/class/const/function
                if let Some(decl) = &export_decl.declaration {
                    match decl {
                        Declaration::TSInterfaceDeclaration(d) => {
                            result
                                .exported
                                .insert(DeclKey::new(owner, d.id.name.as_str()));
                        }
                        Declaration::TSTypeAliasDeclaration(d) => {
                            result
                                .exported
                                .insert(DeclKey::new(owner, d.id.name.as_str()));
                        }
                        Declaration::TSEnumDeclaration(d) => {
                            result
                                .exported
                                .insert(DeclKey::new(owner, d.id.name.as_str()));
                        }
                        Declaration::ClassDeclaration(d) => {
                            if let Some(id) = &d.id {
                                result
                                    .exported
                                    .insert(DeclKey::new(owner, id.name.as_str()));
                            }
                        }
                        Declaration::FunctionDeclaration(d) => {
                            if let Some(id) = &d.id {
                                result
                                    .exported
                                    .insert(DeclKey::new(owner, id.name.as_str()));
                            }
                        }
                        Declaration::VariableDeclaration(d) => {
                            for declarator in &d.declarations {
                                if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                                    result
                                        .exported
                                        .insert(DeclKey::new(owner, id.name.as_str()));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            // export * from './other'
            Statement::ExportAllDeclaration(export_all) => {
                result.wildcard_reexports.push(WildcardReexport {
                    owner,
                    source: export_all.source.value.to_string(),
                    capability: SyntaxCapability::from_export_kind(export_all.export_kind),
                    exported_namespace: export_all
                        .exported
                        .as_ref()
                        .map(|name| DeclKey::new(owner, name.name().to_string())),
                });
            }
            // export default ...
            Statement::ExportDefaultDeclaration(_) => {
                result.exported.insert(DeclKey::new(owner, "default"));
            }
            _ => {}
        }
    }

    result
}

fn record_namespace_type_symbols(
    module: &TSModuleDeclaration<'_>,
    owner: TopLevelOwnerId,
    parent: Option<&DeclarationPath>,
    top_level_carrier: Option<&DeclKey>,
    carrier_span: Span,
    local_type_symbols: &mut BTreeMap<DeclarationPath, AnalyzedExternalTypeSymbol>,
    carriers: &mut BTreeMap<DeclarationPath, Vec<NamespaceTypeCarrier>>,
    with_bodies: bool,
) {
    let TSModuleDeclarationName::Identifier(identifier) = &module.id else {
        return;
    };
    let namespace = parent.map_or_else(
        || DeclarationPath::root(DeclKey::new(owner, identifier.name.as_str())),
        |parent| parent.appended(identifier.name.as_str()),
    );
    let carrier = top_level_carrier
        .cloned()
        .unwrap_or_else(|| namespace.root.clone());
    let Some(body) = module.body.as_ref() else {
        return;
    };
    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => record_namespace_type_symbols(
            inner,
            owner,
            Some(&namespace),
            Some(&carrier),
            carrier_span,
            local_type_symbols,
            carriers,
            with_bodies,
        ),
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for statement in &block.body {
                record_namespaced_statement_type_symbol(
                    statement,
                    &namespace,
                    &carrier,
                    carrier_span,
                    local_type_symbols,
                    carriers,
                    with_bodies,
                );
            }
        }
    }
}

fn record_namespaced_statement_type_symbol(
    statement: &Statement<'_>,
    namespace: &DeclarationPath,
    carrier: &DeclKey,
    carrier_span: Span,
    local_type_symbols: &mut BTreeMap<DeclarationPath, AnalyzedExternalTypeSymbol>,
    carriers: &mut BTreeMap<DeclarationPath, Vec<NamespaceTypeCarrier>>,
    with_bodies: bool,
) {
    let mut record = |name: &str,
                      kind: AnalyzedExternalTypeSymbolKind,
                      span: Span,
                      dependencies: DeclDependencyNames| {
        let declaration = namespace.appended(name);
        insert_or_merge_type_symbol(
            local_type_symbols,
            declaration.clone(),
            AnalyzedExternalTypeSymbol::from_dependencies(kind, span, dependencies),
        );
        carriers
            .entry(declaration.clone())
            .or_default()
            .push(NamespaceTypeCarrier {
                declaration,
                carrier: carrier.clone(),
                carrier_span,
            });
    };
    match statement {
        Statement::TSTypeAliasDeclaration(alias) => record(
            alias.id.name.as_str(),
            AnalyzedExternalTypeSymbolKind::TypeAlias,
            alias.span.into(),
            with_bodies
                .then(|| type_alias_dependency_names(alias))
                .unwrap_or_default(),
        ),
        Statement::TSInterfaceDeclaration(interface) => record(
            interface.id.name.as_str(),
            AnalyzedExternalTypeSymbolKind::Interface,
            interface.span.into(),
            with_bodies
                .then(|| interface_dependency_names(interface))
                .unwrap_or_default(),
        ),
        Statement::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                record(
                    identifier.name.as_str(),
                    AnalyzedExternalTypeSymbolKind::Class,
                    class.span.into(),
                    with_bodies
                        .then(|| class_dependency_names(class))
                        .unwrap_or_default(),
                );
            }
        }
        Statement::TSModuleDeclaration(inner) => record_namespace_type_symbols(
            inner,
            namespace.root.owner,
            Some(namespace),
            Some(carrier),
            carrier_span,
            local_type_symbols,
            carriers,
            with_bodies,
        ),
        Statement::ExportNamedDeclaration(export) => {
            if let Some(declaration) = export.declaration.as_ref() {
                record_namespaced_declaration_type_symbol(
                    declaration,
                    namespace,
                    carrier,
                    carrier_span,
                    local_type_symbols,
                    carriers,
                    with_bodies,
                );
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn record_namespaced_declaration_type_symbol(
    declaration: &Declaration<'_>,
    namespace: &DeclarationPath,
    carrier: &DeclKey,
    carrier_span: Span,
    local_type_symbols: &mut BTreeMap<DeclarationPath, AnalyzedExternalTypeSymbol>,
    carriers: &mut BTreeMap<DeclarationPath, Vec<NamespaceTypeCarrier>>,
    with_bodies: bool,
) {
    match declaration {
        Declaration::TSTypeAliasDeclaration(alias) => {
            let declaration = namespace.appended(alias.id.name.as_str());
            insert_or_merge_type_symbol(
                local_type_symbols,
                declaration.clone(),
                AnalyzedExternalTypeSymbol::from_dependencies(
                    AnalyzedExternalTypeSymbolKind::TypeAlias,
                    alias.span.into(),
                    with_bodies
                        .then(|| type_alias_dependency_names(alias))
                        .unwrap_or_default(),
                ),
            );
            carriers
                .entry(declaration.clone())
                .or_default()
                .push(NamespaceTypeCarrier {
                    declaration,
                    carrier: carrier.clone(),
                    carrier_span,
                });
        }
        Declaration::TSModuleDeclaration(inner) => record_namespace_type_symbols(
            inner,
            namespace.root.owner,
            Some(namespace),
            Some(carrier),
            carrier_span,
            local_type_symbols,
            carriers,
            with_bodies,
        ),
        Declaration::TSInterfaceDeclaration(interface) => {
            let declaration = namespace.appended(interface.id.name.as_str());
            insert_or_merge_type_symbol(
                local_type_symbols,
                declaration.clone(),
                AnalyzedExternalTypeSymbol::from_dependencies(
                    AnalyzedExternalTypeSymbolKind::Interface,
                    interface.span.into(),
                    with_bodies
                        .then(|| interface_dependency_names(interface))
                        .unwrap_or_default(),
                ),
            );
            carriers
                .entry(declaration.clone())
                .or_default()
                .push(NamespaceTypeCarrier {
                    declaration,
                    carrier: carrier.clone(),
                    carrier_span,
                });
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(identifier) = &class.id {
                let declaration = namespace.appended(identifier.name.as_str());
                insert_or_merge_type_symbol(
                    local_type_symbols,
                    declaration.clone(),
                    AnalyzedExternalTypeSymbol::from_dependencies(
                        AnalyzedExternalTypeSymbolKind::Class,
                        class.span.into(),
                        with_bodies
                            .then(|| class_dependency_names(class))
                            .unwrap_or_default(),
                    ),
                );
                carriers
                    .entry(declaration.clone())
                    .or_default()
                    .push(NamespaceTypeCarrier {
                        declaration,
                        carrier: carrier.clone(),
                        carrier_span,
                    });
            }
        }
        _ => {}
    }
}

pub fn collect_required_import_bindings_for_external_type(
    declaration: &DeclarationPath,
    dep_source: &str,
    allocator: &oxc_allocator::Allocator,
) -> BTreeSet<DeclKey> {
    analyze_external_type_source(dep_source, allocator).required_import_bindings(declaration)
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn analyze_external_type_program(program: &Program<'_>) -> AnalyzedExternalTypeSource {
    analyze_external_type_program_for_owner(program, TopLevelOwnerId::ordinary_file())
}

pub fn analyze_external_type_program_for_owner(
    program: &Program<'_>,
    owner: TopLevelOwnerId,
) -> AnalyzedExternalTypeSource {
    analyze_external_type_program_impl(program, OwnerAssignment::Uniform(owner), true)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerTableError {
    statement_count: usize,
    owner_count: usize,
}

impl OwnerTableError {
    #[must_use]
    pub fn statement_count(self) -> usize {
        self.statement_count
    }

    #[must_use]
    pub fn owner_count(self) -> usize {
        self.owner_count
    }
}

pub fn analyze_external_type_program_with_owner_table(
    program: &Program<'_>,
    owners: &[TopLevelOwnerId],
) -> Result<AnalyzedExternalTypeSource, OwnerTableError> {
    validate_owner_table(program, owners)?;
    Ok(analyze_external_type_program_impl(
        program,
        OwnerAssignment::Table(owners),
        true,
    ))
}

/// HEADER-ONLY analyzer variant — the `IndexedReady` artifact producer.
///
/// Identical import/export/reexport/symbol-NAME inventory to
/// [`analyze_external_type_program`], with the per-declaration BODY walks
/// skipped: `local_type_symbols` carry kind + span with EMPTY dependency
/// sets, and no `RawSourceSurface` inventory is captured. Body-derived
/// facts (dependency names, raw surfaces) are demand products of the lazy
/// declaration-body path ([`collect_statement_dependencies`], the
/// per-statement raw-surface capture) — never an eager whole-program walk
/// at artifact publish.
pub fn analyze_external_type_program_headers(program: &Program<'_>) -> AnalyzedExternalTypeSource {
    analyze_external_type_program_headers_for_owner(program, TopLevelOwnerId::ordinary_file())
}

pub fn analyze_external_type_program_headers_for_owner(
    program: &Program<'_>,
    owner: TopLevelOwnerId,
) -> AnalyzedExternalTypeSource {
    analyze_external_type_program_impl(program, OwnerAssignment::Uniform(owner), false)
}

pub fn analyze_external_type_program_headers_with_owner_table(
    program: &Program<'_>,
    owners: &[TopLevelOwnerId],
) -> Result<AnalyzedExternalTypeSource, OwnerTableError> {
    validate_owner_table(program, owners)?;
    Ok(analyze_external_type_program_impl(
        program,
        OwnerAssignment::Table(owners),
        false,
    ))
}

fn validate_owner_table(
    program: &Program<'_>,
    owners: &[TopLevelOwnerId],
) -> Result<(), OwnerTableError> {
    (program.body.len() == owners.len())
        .then_some(())
        .ok_or(OwnerTableError {
            statement_count: program.body.len(),
            owner_count: owners.len(),
        })
}

#[derive(Clone, Copy)]
enum OwnerAssignment<'a> {
    Uniform(TopLevelOwnerId),
    Table(&'a [TopLevelOwnerId]),
}

impl OwnerAssignment<'_> {
    fn at(self, index: usize) -> TopLevelOwnerId {
        match self {
            Self::Uniform(owner) => owner,
            Self::Table(owners) => owners[index],
        }
    }
}

fn capture_owned_statement_surfaces(
    statement: &Statement<'_>,
    owner: TopLevelOwnerId,
    namespace: Option<&DeclarationPath>,
    captures: &mut BTreeMap<DeclarationPath, Vec<CapturedSurface>>,
) {
    let module = match statement {
        Statement::TSModuleDeclaration(module) => Some(module),
        Statement::ExportNamedDeclaration(export) => match export.declaration.as_ref() {
            Some(Declaration::TSModuleDeclaration(module)) => Some(module),
            _ => None,
        },
        _ => None,
    };
    if let Some(module) = module {
        capture_owned_module_surfaces(module, owner, namespace, captures);
        return;
    }

    for captured in capture_statement_surfaces(statement) {
        let declaration = namespace.map_or_else(
            || DeclarationPath::root(DeclKey::new(owner, captured.name.as_str())),
            |namespace| namespace.appended(captured.name.as_str()),
        );
        captures.entry(declaration).or_default().push(captured);
    }
}

fn capture_owned_module_surfaces(
    module: &TSModuleDeclaration<'_>,
    owner: TopLevelOwnerId,
    parent: Option<&DeclarationPath>,
    captures: &mut BTreeMap<DeclarationPath, Vec<CapturedSurface>>,
) {
    let TSModuleDeclarationName::Identifier(identifier) = &module.id else {
        return;
    };
    let namespace = parent.map_or_else(
        || DeclarationPath::root(DeclKey::new(owner, identifier.name.as_str())),
        |parent| parent.appended(identifier.name.as_str()),
    );
    let Some(body) = module.body.as_ref() else {
        return;
    };
    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
            capture_owned_module_surfaces(inner, owner, Some(&namespace), captures);
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for statement in &block.body {
                capture_owned_statement_surfaces(statement, owner, Some(&namespace), captures);
            }
        }
    }
}

fn analyze_external_type_program_impl(
    program: &Program<'_>,
    owners: OwnerAssignment<'_>,
    with_bodies: bool,
) -> AnalyzedExternalTypeSource {
    let mut result = AnalyzedExternalTypeSource::default();
    let mut raw_captures = BTreeMap::new();

    for (statement_index, stmt) in program.body.iter().enumerate() {
        let owner = owners.at(statement_index);
        result.top_level_statement_count += 1;
        if with_bodies {
            capture_owned_statement_surfaces(stmt, owner, None, &mut raw_captures);
        }
        match stmt {
            Statement::ImportDeclaration(import_decl) => {
                let Some(specifiers) = &import_decl.specifiers else {
                    result
                        .extracted
                        .bindingless_imports
                        .push(BindinglessImport {
                            owner,
                            source: import_decl.source.value.to_string(),
                        });
                    continue;
                };
                if specifiers.is_empty() {
                    result
                        .extracted
                        .bindingless_imports
                        .push(BindinglessImport {
                            owner,
                            source: import_decl.source.value.to_string(),
                        });
                    continue;
                }
                for specifier in specifiers {
                    let binding = match specifier {
                        ImportDeclarationSpecifier::ImportSpecifier(import_spec) => {
                            let capability = if import_decl.import_kind == ImportOrExportKind::Type
                                || import_spec.import_kind == ImportOrExportKind::Type
                            {
                                SyntaxCapability::TypeOnly
                            } else {
                                SyntaxCapability::TypeAndValue
                            };
                            ImportedTypeBinding {
                                local: DeclKey::new(owner, import_spec.local.name.as_str()),
                                source: import_decl.source.value.to_string(),
                                form: ImportBindingForm::Named,
                                capability,
                                imported: ImportedExportPath::Symbol(
                                    TypeDependencyPathFact::from_segments([import_spec
                                        .imported
                                        .name()
                                        .to_string()])
                                    .expect("module export name is non-empty"),
                                ),
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(import_spec) => {
                            ImportedTypeBinding {
                                local: DeclKey::new(owner, import_spec.local.name.as_str()),
                                source: import_decl.source.value.to_string(),
                                form: ImportBindingForm::Default,
                                capability: SyntaxCapability::from_import_kind(
                                    import_decl.import_kind,
                                ),
                                imported: ImportedExportPath::Symbol(
                                    TypeDependencyPathFact::from_segments(["default"])
                                        .expect("default export path is non-empty"),
                                ),
                            }
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(import_spec) => {
                            ImportedTypeBinding {
                                local: DeclKey::new(owner, import_spec.local.name.as_str()),
                                source: import_decl.source.value.to_string(),
                                form: ImportBindingForm::Namespace,
                                capability: SyntaxCapability::from_import_kind(
                                    import_decl.import_kind,
                                ),
                                imported: ImportedExportPath::NamespaceRoot,
                            }
                        }
                    };
                    result.extracted.bindings.push(binding.clone());
                    insert_unique(&mut result.imports, binding.local.clone(), binding);
                }
            }
            Statement::ExportNamedDeclaration(export_decl) => {
                if let Some(source) = &export_decl.source {
                    for specifier in &export_decl.specifiers {
                        let capability = if export_decl.export_kind == ImportOrExportKind::Type
                            || specifier.export_kind == ImportOrExportKind::Type
                        {
                            SyntaxCapability::TypeOnly
                        } else {
                            SyntaxCapability::TypeAndValue
                        };
                        let target = ExportTarget::External(RoutedImportTarget {
                            source: source.value.to_string(),
                            form: ImportBindingForm::Named,
                            capability,
                            exported: ImportedExportPath::Symbol(
                                TypeDependencyPathFact::from_segments([specifier
                                    .local
                                    .name()
                                    .to_string()])
                                .expect("module export name is non-empty"),
                            ),
                        });
                        let exported = DeclKey::new(owner, specifier.exported.name().to_string());
                        result.extracted.reexport_bindings.push(ReexportBinding {
                            exported: exported.clone(),
                            target: target.clone(),
                        });
                        insert_unique(&mut result.exports, exported, target);
                    }
                    continue;
                }

                for specifier in &export_decl.specifiers {
                    let exported = DeclKey::new(owner, specifier.exported.name().to_string());
                    let capability = if export_decl.export_kind == ImportOrExportKind::Type
                        || specifier.export_kind == ImportOrExportKind::Type
                    {
                        SyntaxCapability::TypeOnly
                    } else {
                        SyntaxCapability::TypeAndValue
                    };
                    let target = ExportTarget::LocalBinding {
                        binding: DeclKey::new(owner, specifier.local.name().to_string()),
                        capability,
                    };
                    result.extracted.reexport_bindings.push(ReexportBinding {
                        exported: exported.clone(),
                        target: target.clone(),
                    });
                    insert_unique(&mut result.exports, exported, target);
                }

                if let Some(declaration) = &export_decl.declaration {
                    if let Declaration::TSModuleDeclaration(module) = declaration {
                        record_namespace_type_symbols(
                            module,
                            owner,
                            None,
                            None,
                            export_decl.span.into(),
                            &mut result.local_type_symbols,
                            &mut result.namespace_type_carriers,
                            with_bodies,
                        );
                    }
                    record_local_export_symbol_targets_from_declaration(
                        declaration,
                        owner,
                        &mut result.exports,
                    );
                    record_local_type_symbol_from_declaration(
                        declaration,
                        owner,
                        &mut result.local_type_symbols,
                        with_bodies,
                    );
                    record_exported_local_type_declarations(
                        declaration,
                        owner,
                        &mut result.exported_local_type_declarations,
                    );
                }
            }
            Statement::ExportAllDeclaration(export_all) => {
                let capability = SyntaxCapability::from_export_kind(export_all.export_kind);
                let exported_namespace = export_all
                    .exported
                    .as_ref()
                    .map(|name| DeclKey::new(owner, name.name().to_string()));
                let row = WildcardReexport {
                    owner,
                    source: export_all.source.value.to_string(),
                    capability,
                    exported_namespace: exported_namespace.clone(),
                };
                if let Some(exported) = exported_namespace {
                    let target = ExportTarget::External(RoutedImportTarget {
                        source: row.source.clone(),
                        form: ImportBindingForm::Namespace,
                        capability,
                        exported: ImportedExportPath::NamespaceRoot,
                    });
                    result.extracted.reexport_bindings.push(ReexportBinding {
                        exported: exported.clone(),
                        target: target.clone(),
                    });
                    insert_unique(&mut result.exports, exported, target);
                }
                result.extracted.wildcard_reexports.push(row);
            }
            // `export = X` — a CommonJS-style export assignment. Capture the
            // assigned local VALUE name so `typeof import("./m")` can resolve
            // the whole module to `typeof X`. Only a bare identifier target is
            // captured (the `export = SomeValue` form); a non-identifier
            // expression has no addressable value root and is left `None`.
            Statement::TSExportAssignment(assignment) => {
                if let Expression::Identifier(ident) = &assignment.expression {
                    insert_strict(
                        &mut result.export_assignment_targets,
                        owner,
                        DeclKey::new(owner, ident.name.as_str()),
                    );
                }
            }
            Statement::TSTypeAliasDeclaration(type_alias) => {
                let deps = if with_bodies {
                    type_alias_dependency_names(type_alias)
                } else {
                    DeclDependencyNames::default()
                };
                insert_or_merge_type_symbol(
                    &mut result.local_type_symbols,
                    DeclarationPath::root(DeclKey::new(owner, type_alias.id.name.as_str())),
                    AnalyzedExternalTypeSymbol::from_dependencies(
                        AnalyzedExternalTypeSymbolKind::TypeAlias,
                        type_alias.span.into(),
                        deps,
                    ),
                );
            }
            Statement::TSInterfaceDeclaration(interface) => {
                let deps = if with_bodies {
                    interface_dependency_names(interface)
                } else {
                    DeclDependencyNames::default()
                };
                insert_or_merge_type_symbol(
                    &mut result.local_type_symbols,
                    DeclarationPath::root(DeclKey::new(owner, interface.id.name.as_str())),
                    AnalyzedExternalTypeSymbol::from_dependencies(
                        AnalyzedExternalTypeSymbolKind::Interface,
                        interface.span.into(),
                        deps,
                    ),
                );
            }
            Statement::ClassDeclaration(class_decl) => {
                if let Some(id) = &class_decl.id {
                    let deps = if with_bodies {
                        class_dependency_names(class_decl)
                    } else {
                        DeclDependencyNames::default()
                    };
                    insert_or_merge_type_symbol(
                        &mut result.local_type_symbols,
                        DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                        AnalyzedExternalTypeSymbol::from_dependencies(
                            AnalyzedExternalTypeSymbolKind::Class,
                            class_decl.span.into(),
                            deps,
                        ),
                    );
                }
            }
            Statement::TSModuleDeclaration(module) => {
                record_namespace_type_symbols(
                    module,
                    owner,
                    None,
                    None,
                    module.span.into(),
                    &mut result.local_type_symbols,
                    &mut result.namespace_type_carriers,
                    with_bodies,
                );
            }
            Statement::ExportDefaultDeclaration(export_default) => {
                match &export_default.declaration {
                    ExportDefaultDeclarationKind::ClassDeclaration(class_decl) => {
                        let deps = if with_bodies {
                            class_dependency_names(class_decl)
                        } else {
                            DeclDependencyNames::default()
                        };
                        let declaration = class_decl.id.as_ref().map_or_else(
                            || DeclarationPath::root(DeclKey::new(owner, "default")),
                            |id| DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                        );
                        insert_or_merge_type_symbol(
                            &mut result.local_type_symbols,
                            declaration.clone(),
                            AnalyzedExternalTypeSymbol::from_dependencies(
                                AnalyzedExternalTypeSymbolKind::Class,
                                export_default.span.into(),
                                deps,
                            ),
                        );
                        result
                            .exported_local_type_declarations
                            .insert(declaration.clone());
                        insert_local_declaration_export(
                            &mut result.exports,
                            DeclKey::new(owner, "default"),
                            declaration,
                            SyntaxCapability::TypeAndValue,
                        );
                    }
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                        let deps = if with_bodies {
                            interface_dependency_names(interface)
                        } else {
                            DeclDependencyNames::default()
                        };
                        let declaration =
                            DeclarationPath::root(DeclKey::new(owner, interface.id.name.as_str()));
                        insert_or_merge_type_symbol(
                            &mut result.local_type_symbols,
                            declaration.clone(),
                            AnalyzedExternalTypeSymbol::from_dependencies(
                                AnalyzedExternalTypeSymbolKind::Interface,
                                export_default.span.into(),
                                deps,
                            ),
                        );
                        result
                            .exported_local_type_declarations
                            .insert(declaration.clone());
                        insert_local_declaration_export(
                            &mut result.exports,
                            DeclKey::new(owner, "default"),
                            declaration,
                            SyntaxCapability::TypeOnly,
                        );
                    }
                    // `export default <ident>` (e.g. `export default leafDefault`)
                    // — the default export's VALUE is the referenced local
                    // binding. Map the `default` export name to that local value
                    // name so `typeof import("./m").default` resolves to
                    // `typeof <ident>` (the export-target chase reaches the
                    // local value decl).
                    ExportDefaultDeclarationKind::Identifier(ident) => {
                        insert_unique(
                            &mut result.exports,
                            DeclKey::new(owner, "default"),
                            ExportTarget::LocalBinding {
                                binding: DeclKey::new(owner, ident.name.as_str()),
                                capability: SyntaxCapability::ValueOnly,
                            },
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    reconcile_export_targets(&mut result);

    // Oracle harness: capture the parse-time `RawSourceSurface` raw-fact
    // inventory while the OXC arena is still live. BODY data — captured
    // only on the with-bodies path; the header-only artifact analyzer
    // leaves the inventory empty (per-symbol raw surfaces are demand
    // products of the lazy declaration-body memo).
    if with_bodies {
        for (declaration, captured) in raw_captures {
            for c in merge_overload_groups(captured) {
                result
                    .raw_source_surfaces
                    .entry((declaration.clone(), c.symbol_space))
                    .or_default()
                    .push(c.surface);
            }
        }
    }

    result
}

fn record_local_type_symbol_from_declaration(
    declaration: &Declaration<'_>,
    owner: TopLevelOwnerId,
    local_type_symbols: &mut BTreeMap<DeclarationPath, AnalyzedExternalTypeSymbol>,
    with_bodies: bool,
) {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => {
            let deps = if with_bodies {
                type_alias_dependency_names(type_alias)
            } else {
                DeclDependencyNames::default()
            };
            insert_or_merge_type_symbol(
                local_type_symbols,
                DeclarationPath::root(DeclKey::new(owner, type_alias.id.name.as_str())),
                AnalyzedExternalTypeSymbol::from_dependencies(
                    AnalyzedExternalTypeSymbolKind::TypeAlias,
                    type_alias.span.into(),
                    deps,
                ),
            );
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            let deps = if with_bodies {
                interface_dependency_names(interface)
            } else {
                DeclDependencyNames::default()
            };
            insert_or_merge_type_symbol(
                local_type_symbols,
                DeclarationPath::root(DeclKey::new(owner, interface.id.name.as_str())),
                AnalyzedExternalTypeSymbol::from_dependencies(
                    AnalyzedExternalTypeSymbolKind::Interface,
                    interface.span.into(),
                    deps,
                ),
            );
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                let deps = if with_bodies {
                    class_dependency_names(class_decl)
                } else {
                    DeclDependencyNames::default()
                };
                insert_or_merge_type_symbol(
                    local_type_symbols,
                    DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                    AnalyzedExternalTypeSymbol::from_dependencies(
                        AnalyzedExternalTypeSymbolKind::Class,
                        class_decl.span.into(),
                        deps,
                    ),
                );
            }
        }
        _ => {}
    }
}

fn record_exported_local_type_declarations(
    declaration: &Declaration<'_>,
    owner: TopLevelOwnerId,
    exported_local_types: &mut BTreeSet<DeclarationPath>,
) {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => {
            exported_local_types.insert(DeclarationPath::root(DeclKey::new(
                owner,
                type_alias.id.name.as_str(),
            )));
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            exported_local_types.insert(DeclarationPath::root(DeclKey::new(
                owner,
                interface.id.name.as_str(),
            )));
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                exported_local_types
                    .insert(DeclarationPath::root(DeclKey::new(owner, id.name.as_str())));
            }
        }
        Declaration::TSModuleDeclaration(module) => {
            if let TSModuleDeclarationName::Identifier(id) = &module.id {
                exported_local_types
                    .insert(DeclarationPath::root(DeclKey::new(owner, id.name.as_str())));
            }
        }
        _ => {}
    }
}

fn record_local_export_symbol_targets_from_declaration(
    declaration: &Declaration<'_>,
    owner: TopLevelOwnerId,
    exports: &mut BTreeMap<DeclKey, UniqueResolution<ExportTarget>>,
) {
    let mut record = |name: &str, declaration: DeclarationPath, capability: SyntaxCapability| {
        insert_local_declaration_export(
            exports,
            DeclKey::new(owner, name),
            declaration,
            capability,
        );
    };
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => {
            record(
                type_alias.id.name.as_str(),
                DeclarationPath::root(DeclKey::new(owner, type_alias.id.name.as_str())),
                SyntaxCapability::TypeOnly,
            );
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            record(
                interface.id.name.as_str(),
                DeclarationPath::root(DeclKey::new(owner, interface.id.name.as_str())),
                SyntaxCapability::TypeOnly,
            );
        }
        Declaration::TSEnumDeclaration(enum_decl) => {
            record(
                enum_decl.id.name.as_str(),
                DeclarationPath::root(DeclKey::new(owner, enum_decl.id.name.as_str())),
                SyntaxCapability::TypeAndValue,
            );
        }
        Declaration::ClassDeclaration(class_decl) => {
            if let Some(id) = &class_decl.id {
                record(
                    id.name.as_str(),
                    DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                    SyntaxCapability::TypeAndValue,
                );
            }
        }
        Declaration::FunctionDeclaration(function_decl) => {
            if let Some(id) = &function_decl.id {
                record(
                    id.name.as_str(),
                    DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                    SyntaxCapability::ValueOnly,
                );
            }
        }
        Declaration::VariableDeclaration(variable_decl) => {
            for declarator in &variable_decl.declarations {
                let BindingPattern::BindingIdentifier(id) = &declarator.id else {
                    continue;
                };
                record(
                    id.name.as_str(),
                    DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                    SyntaxCapability::ValueOnly,
                );
            }
        }
        Declaration::TSModuleDeclaration(module) => {
            if let TSModuleDeclarationName::Identifier(id) = &module.id {
                record(
                    id.name.as_str(),
                    DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                    SyntaxCapability::TypeAndValue,
                );
            }
        }
        _ => {}
    }
}

/// The reference-name pair one declaration's BODY contributes: the plain
/// dependency names plus the structural subset. This is the per-statement
/// demand product the lazy declaration-body path consumes — computed for
/// exactly the demanded declaration, never for every symbol in the file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclDependencyNames {
    /// Parser-owned segment identities. Consumers classify the local binding
    /// from `segments[0]` and preserve the remaining member path separately;
    /// no downstream string splitting is permitted.
    pub dependency_paths: BTreeSet<TypeDependencyPathFact>,
    pub structural_dependency_paths: BTreeSet<TypeDependencyPathFact>,
    /// Complete declaration-carrier dependencies, including return types,
    /// generic bounds/defaults, constructors, and static members. Kept
    /// separate so legacy component-meta dependency breadth is unchanged.
    pub declaration_carrier_paths: BTreeSet<TypeDependencyPathFact>,
    /// Runtime value roles carried by declaration syntax.
    pub value_query_paths: BTreeSet<TypeDependencyPathFact>,
    pub value_position_paths: BTreeSet<TypeDependencyPathFact>,
    pub unsupported_value_positions: BTreeSet<UnsupportedValuePositionKind>,
}

/// Collect the complete parser-owned dependency paths of one authored type
/// expression. This is the shared macro/declaration carrier walker; consumers
/// receive segment identities and never reconstruct qualified names.
pub fn collect_type_dependency_paths(ts_type: &TSType<'_>) -> BTreeSet<TypeDependencyPathFact> {
    let mut out = DeclDependencyNames::default();
    let mut collector = TypeDependencyCollector::new(
        &mut out.dependency_paths,
        &mut out.structural_dependency_paths,
        &mut out.declaration_carrier_paths,
        &mut out.value_query_paths,
        &mut out.value_position_paths,
        &mut out.unsupported_value_positions,
    );
    collector.visit_type(ts_type, StructuralDependencyContext::Root);
    out.declaration_carrier_paths
}

/// Re-locate one analyzer-addressed macro call and collect its first authored
/// type argument through the shared typed dependency walker.
pub fn collect_macro_type_dependency_paths_at_span(
    program: &Program<'_>,
    macro_span: verter_span::Span,
) -> Option<BTreeSet<TypeDependencyPathFact>> {
    fn from_call(
        call: &CallExpression<'_>,
        macro_span: verter_span::Span,
    ) -> Option<BTreeSet<TypeDependencyPathFact>> {
        if verter_span::Span::from(call.span) == macro_span {
            let first = call.type_arguments.as_ref()?.params.first()?;
            return Some(collect_type_dependency_paths(first));
        }
        call.arguments.iter().find_map(|argument| {
            let Expression::CallExpression(inner) = argument.as_expression()? else {
                return None;
            };
            from_call(inner, macro_span)
        })
    }

    program.body.iter().find_map(|statement| match statement {
        Statement::ExpressionStatement(statement) => {
            let Expression::CallExpression(call) = &statement.expression else {
                return None;
            };
            from_call(call, macro_span)
        }
        Statement::VariableDeclaration(declaration) => {
            declaration.declarations.iter().find_map(|declarator| {
                let Expression::CallExpression(call) = declarator.init.as_ref()? else {
                    return None;
                };
                from_call(call, macro_span)
            })
        }
        _ => None,
    })
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
pub fn collect_statement_dependencies(
    stmt: &Statement<'_>,
    owner: TopLevelOwnerId,
) -> Vec<(DeclarationPath, DeclDependencyNames)> {
    match stmt {
        Statement::TSTypeAliasDeclaration(type_alias) => vec![(
            DeclarationPath::root(DeclKey::new(owner, type_alias.id.name.as_str())),
            type_alias_dependency_names(type_alias),
        )],
        Statement::TSInterfaceDeclaration(interface) => vec![(
            DeclarationPath::root(DeclKey::new(owner, interface.id.name.as_str())),
            interface_dependency_names(interface),
        )],
        Statement::ClassDeclaration(class_decl) => class_decl
            .id
            .as_ref()
            .map(|id| {
                vec![(
                    DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                    class_dependency_names(class_decl),
                )]
            })
            .unwrap_or_default(),
        Statement::TSModuleDeclaration(module) => {
            // An identifier namespace registers its inner type declarations
            // under qualified `Ns.Name` keys (matching `lower_top_level_
            // statement`'s `extract_module_declaration`); a string-literal
            // ambient module is an augmentation scope whose dep edges ride
            // on the per-contributor `FileWholeHash` rail — nothing to
            // collect here.
            let mut out = Vec::new();
            collect_module_dependencies(module, owner, None, &mut out);
            out
        }
        Statement::ExportNamedDeclaration(export) => export
            .declaration
            .as_ref()
            .map(|declaration| collect_declaration_dependencies(declaration, owner))
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
                        (
                            DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                            deps.clone(),
                        ),
                        (DeclarationPath::root(DeclKey::new(owner, "default")), deps),
                    ],
                    None => vec![(DeclarationPath::root(DeclKey::new(owner, "default")), deps)],
                }
            }
            ExportDefaultDeclarationKind::TSInterfaceDeclaration(interface) => {
                let deps = interface_dependency_names(interface);
                vec![
                    (
                        DeclarationPath::root(DeclKey::new(owner, interface.id.name.as_str())),
                        deps.clone(),
                    ),
                    (DeclarationPath::root(DeclKey::new(owner, "default")), deps),
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
fn collect_module_dependencies(
    module: &TSModuleDeclaration<'_>,
    owner: TopLevelOwnerId,
    parent: Option<&DeclarationPath>,
    out: &mut Vec<(DeclarationPath, DeclDependencyNames)>,
) {
    let namespace = match &module.id {
        TSModuleDeclarationName::Identifier(id) => parent.map_or_else(
            || DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
            |parent| parent.appended(id.name.as_str()),
        ),
        TSModuleDeclarationName::StringLiteral(_) => return,
    };
    let Some(body) = module.body.as_ref() else {
        return;
    };
    match body {
        TSModuleDeclarationBody::TSModuleDeclaration(inner) => {
            collect_module_dependencies(inner, owner, Some(&namespace), out);
        }
        TSModuleDeclarationBody::TSModuleBlock(block) => {
            for stmt in &block.body {
                collect_namespaced_statement_dependencies(stmt, &namespace, out);
            }
        }
    }
}

fn collect_namespaced_statement_dependencies(
    stmt: &Statement<'_>,
    namespace: &DeclarationPath,
    out: &mut Vec<(DeclarationPath, DeclDependencyNames)>,
) {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => {
            out.push((
                namespace.appended(alias.id.name.as_str()),
                type_alias_dependency_names(alias),
            ));
        }
        Statement::TSInterfaceDeclaration(interface) => {
            out.push((
                namespace.appended(interface.id.name.as_str()),
                interface_dependency_names(interface),
            ));
        }
        Statement::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                out.push((
                    namespace.appended(id.name.as_str()),
                    class_dependency_names(class),
                ));
            }
        }
        Statement::TSModuleDeclaration(module) => {
            collect_module_dependencies(module, namespace.root.owner, Some(namespace), out);
        }
        Statement::ExportNamedDeclaration(export) => {
            if let Some(decl) = export.declaration.as_ref() {
                collect_namespaced_declaration_dependencies(decl, namespace, out);
            }
        }
        _ => {}
    }
}

fn collect_namespaced_declaration_dependencies(
    decl: &Declaration<'_>,
    namespace: &DeclarationPath,
    out: &mut Vec<(DeclarationPath, DeclDependencyNames)>,
) {
    match decl {
        Declaration::TSTypeAliasDeclaration(alias) => {
            out.push((
                namespace.appended(alias.id.name.as_str()),
                type_alias_dependency_names(alias),
            ));
        }
        Declaration::TSInterfaceDeclaration(interface) => {
            out.push((
                namespace.appended(interface.id.name.as_str()),
                interface_dependency_names(interface),
            ));
        }
        Declaration::ClassDeclaration(class) => {
            if let Some(id) = &class.id {
                out.push((
                    namespace.appended(id.name.as_str()),
                    class_dependency_names(class),
                ));
            }
        }
        Declaration::TSModuleDeclaration(module) => {
            collect_module_dependencies(module, namespace.root.owner, Some(namespace), out);
        }
        _ => {}
    }
}

fn collect_declaration_dependencies(
    declaration: &Declaration<'_>,
    owner: TopLevelOwnerId,
) -> Vec<(DeclarationPath, DeclDependencyNames)> {
    match declaration {
        Declaration::TSTypeAliasDeclaration(type_alias) => vec![(
            DeclarationPath::root(DeclKey::new(owner, type_alias.id.name.as_str())),
            type_alias_dependency_names(type_alias),
        )],
        Declaration::TSInterfaceDeclaration(interface) => vec![(
            DeclarationPath::root(DeclKey::new(owner, interface.id.name.as_str())),
            interface_dependency_names(interface),
        )],
        Declaration::ClassDeclaration(class_decl) => class_decl
            .id
            .as_ref()
            .map(|id| {
                vec![(
                    DeclarationPath::root(DeclKey::new(owner, id.name.as_str())),
                    class_dependency_names(class_decl),
                )]
            })
            .unwrap_or_default(),
        Declaration::TSModuleDeclaration(module) => {
            // `export namespace N { … }` — collect its inner type
            // declarations under qualified `N.Name` keys.
            let mut out = Vec::new();
            collect_module_dependencies(module, owner, None, &mut out);
            out
        }
        _ => Vec::new(),
    }
}

fn type_alias_dependency_names(type_alias: &TSTypeAliasDeclaration<'_>) -> DeclDependencyNames {
    let mut out = DeclDependencyNames::default();
    let mut collector = TypeDependencyCollector::new(
        &mut out.dependency_paths,
        &mut out.structural_dependency_paths,
        &mut out.declaration_carrier_paths,
        &mut out.value_query_paths,
        &mut out.value_position_paths,
        &mut out.unsupported_value_positions,
    );
    collector.visit_type(
        &type_alias.type_annotation,
        StructuralDependencyContext::Root,
    );
    if let Some(parameters) = &type_alias.type_parameters {
        collector.visit_type_parameters_for_carrier(parameters);
    }
    out
}

/// Collect typed dependency facts for a standalone authored type payload.
/// This is also the JSDoc synthetic-alias bridge: callers invoke it while the
/// wrapper OXC arena is alive, before lowering erases qualified path identity.
pub fn collect_type_dependency_facts(ts_type: &TSType<'_>) -> DeclDependencyNames {
    let mut out = DeclDependencyNames::default();
    TypeDependencyCollector::new(
        &mut out.dependency_paths,
        &mut out.structural_dependency_paths,
        &mut out.declaration_carrier_paths,
        &mut out.value_query_paths,
        &mut out.value_position_paths,
        &mut out.unsupported_value_positions,
    )
    .visit_type(ts_type, StructuralDependencyContext::Root);
    out
}

fn interface_dependency_names(interface: &TSInterfaceDeclaration<'_>) -> DeclDependencyNames {
    let mut out = DeclDependencyNames::default();
    let mut collector = TypeDependencyCollector::new(
        &mut out.dependency_paths,
        &mut out.structural_dependency_paths,
        &mut out.declaration_carrier_paths,
        &mut out.value_query_paths,
        &mut out.value_position_paths,
        &mut out.unsupported_value_positions,
    );
    collector.visit_interface(&interface.body.body, &interface.extends);
    if let Some(parameters) = &interface.type_parameters {
        collector.visit_type_parameters_for_carrier(parameters);
    }
    out
}

fn class_dependency_names(class_decl: &Class<'_>) -> DeclDependencyNames {
    let mut out = DeclDependencyNames::default();
    TypeDependencyCollector::new(
        &mut out.dependency_paths,
        &mut out.structural_dependency_paths,
        &mut out.declaration_carrier_paths,
        &mut out.value_query_paths,
        &mut out.value_position_paths,
        &mut out.unsupported_value_positions,
    )
    .visit_class(class_decl);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StructuralDependencyContext {
    Root,
    CallableParam,
    LeafProperty,
    CarrierOnly,
}

fn dependency_path_from_type_name(name: &TSTypeName<'_>) -> Option<TypeDependencyPathFact> {
    fn append(name: &TSTypeName<'_>, segments: &mut Vec<String>) -> bool {
        match name {
            TSTypeName::IdentifierReference(identifier) => {
                segments.push(identifier.name.to_string());
                true
            }
            TSTypeName::QualifiedName(qualified) => {
                if !append(&qualified.left, segments) {
                    return false;
                }
                segments.push(qualified.right.name.to_string());
                true
            }
            TSTypeName::ThisExpression(_) => false,
        }
    }
    let mut segments = Vec::new();
    append(name, &mut segments)
        .then(|| TypeDependencyPathFact::from_segments(segments))
        .flatten()
}

fn dependency_path_from_query_name(
    name: &TSTypeQueryExprName<'_>,
) -> Option<TypeDependencyPathFact> {
    match name {
        TSTypeQueryExprName::IdentifierReference(identifier) => {
            TypeDependencyPathFact::from_segments([identifier.name.to_string()])
        }
        TSTypeQueryExprName::QualifiedName(qualified) => {
            let base = dependency_path_from_type_name(&qualified.left)?;
            let mut segments = base.segments().to_vec();
            segments.push(qualified.right.name.to_string());
            TypeDependencyPathFact::from_segments(segments)
        }
        TSTypeQueryExprName::ThisExpression(_) | TSTypeQueryExprName::TSImportType(_) => None,
    }
}

fn dependency_path_from_expression(expression: &Expression<'_>) -> Option<TypeDependencyPathFact> {
    fn append(expression: &Expression<'_>, segments: &mut Vec<String>) -> bool {
        match expression {
            Expression::Identifier(identifier) => {
                segments.push(identifier.name.to_string());
                true
            }
            Expression::StaticMemberExpression(member) => {
                if !append(&member.object, segments) {
                    return false;
                }
                segments.push(member.property.name.to_string());
                true
            }
            _ => false,
        }
    }
    let mut segments = Vec::new();
    append(expression, &mut segments)
        .then(|| TypeDependencyPathFact::from_segments(segments))
        .flatten()
}

fn dependency_path_from_property_key(key: &PropertyKey<'_>) -> Option<TypeDependencyPathFact> {
    match key {
        PropertyKey::Identifier(identifier) => {
            TypeDependencyPathFact::from_segments([identifier.name.to_string()])
        }
        PropertyKey::StaticIdentifier(identifier) => {
            TypeDependencyPathFact::from_segments([identifier.name.to_string()])
        }
        PropertyKey::StaticMemberExpression(member) => {
            let base = dependency_path_from_expression(&member.object)?;
            let mut segments = base.segments().to_vec();
            segments.push(member.property.name.to_string());
            TypeDependencyPathFact::from_segments(segments)
        }
        _ => None,
    }
}

struct TypeDependencyCollector<'a> {
    full: &'a mut BTreeSet<TypeDependencyPathFact>,
    structural: &'a mut BTreeSet<TypeDependencyPathFact>,
    declaration_carrier: &'a mut BTreeSet<TypeDependencyPathFact>,
    value_queries: &'a mut BTreeSet<TypeDependencyPathFact>,
    value_positions: &'a mut BTreeSet<TypeDependencyPathFact>,
    unsupported_value_positions: &'a mut BTreeSet<UnsupportedValuePositionKind>,
}

impl<'a> TypeDependencyCollector<'a> {
    fn new(
        full: &'a mut BTreeSet<TypeDependencyPathFact>,
        structural: &'a mut BTreeSet<TypeDependencyPathFact>,
        declaration_carrier: &'a mut BTreeSet<TypeDependencyPathFact>,
        value_queries: &'a mut BTreeSet<TypeDependencyPathFact>,
        value_positions: &'a mut BTreeSet<TypeDependencyPathFact>,
        unsupported_value_positions: &'a mut BTreeSet<UnsupportedValuePositionKind>,
    ) -> Self {
        Self {
            full,
            structural,
            declaration_carrier,
            value_queries,
            value_positions,
            unsupported_value_positions,
        }
    }

    fn record(
        &mut self,
        fact: Option<TypeDependencyPathFact>,
        context: StructuralDependencyContext,
    ) {
        let Some(fact) = fact else {
            return;
        };
        self.declaration_carrier.insert(fact.clone());
        if context != StructuralDependencyContext::CarrierOnly {
            self.full.insert(fact.clone());
        }
        if matches!(
            context,
            StructuralDependencyContext::Root | StructuralDependencyContext::CallableParam
        ) {
            self.structural.insert(fact);
        }
    }

    fn record_expression_path(
        &mut self,
        expression: &Expression<'_>,
        context: StructuralDependencyContext,
    ) {
        let fact = dependency_path_from_expression(expression);
        let legacy_context = match expression {
            Expression::Identifier(_) => context,
            _ => StructuralDependencyContext::CarrierOnly,
        };
        self.record(fact, legacy_context);
    }

    fn visit_type_parameters_for_carrier(&mut self, parameters: &TSTypeParameterDeclaration<'_>) {
        for parameter in &parameters.params {
            if let Some(constraint) = &parameter.constraint {
                self.visit_type(constraint, StructuralDependencyContext::CarrierOnly);
            }
            if let Some(default) = &parameter.default {
                self.visit_type(default, StructuralDependencyContext::CarrierOnly);
            }
        }
    }

    fn visit_computed_key(
        &mut self,
        key: &PropertyKey<'_>,
        computed: bool,
        unsupported: UnsupportedValuePositionKind,
    ) {
        if !computed {
            return;
        }
        let path = dependency_path_from_property_key(key);
        if let Some(path) = &path {
            self.value_positions.insert(path.clone());
        } else {
            self.unsupported_value_positions.insert(unsupported);
        }
        self.record(path, StructuralDependencyContext::CarrierOnly);
    }

    fn visit_parameters(
        &mut self,
        parameters: &FormalParameters<'_>,
        context: StructuralDependencyContext,
    ) {
        // Component-meta only needs callable parameter surfaces for
        // props/emits/slots. Return-only imports remain carrier-only.
        for parameter in &parameters.items {
            if let Some(annotation) = &parameter.type_annotation {
                self.visit_type(&annotation.type_annotation, context);
            }
        }
        if let Some(rest) = &parameters.rest {
            if let Some(annotation) = &rest.type_annotation {
                self.visit_type(
                    &annotation.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
        }
    }

    fn visit_this_parameter(&mut self, parameter: Option<&TSThisParameter<'_>>) {
        if let Some(annotation) = parameter.and_then(|parameter| parameter.type_annotation.as_ref())
        {
            self.visit_type(
                &annotation.type_annotation,
                StructuralDependencyContext::CarrierOnly,
            );
        }
    }

    fn visit_index_parameters(&mut self, parameters: &[TSIndexSignatureName<'_>]) {
        for parameter in parameters {
            self.visit_type(
                &parameter.type_annotation.type_annotation,
                StructuralDependencyContext::CarrierOnly,
            );
        }
    }

    fn visit_signatures(&mut self, members: &[TSSignature<'_>], carrier_only: bool) {
        let leaf_context = if carrier_only {
            StructuralDependencyContext::CarrierOnly
        } else {
            StructuralDependencyContext::LeafProperty
        };
        let callable_context = if carrier_only {
            StructuralDependencyContext::CarrierOnly
        } else {
            StructuralDependencyContext::CallableParam
        };
        for member in members {
            match member {
                TSSignature::TSPropertySignature(property) => {
                    self.visit_computed_key(
                        &property.key,
                        property.computed,
                        UnsupportedValuePositionKind::ComputedSignatureKey,
                    );
                    if let Some(annotation) = &property.type_annotation {
                        self.visit_type(&annotation.type_annotation, leaf_context);
                    }
                }
                TSSignature::TSMethodSignature(method) => {
                    self.visit_computed_key(
                        &method.key,
                        method.computed,
                        UnsupportedValuePositionKind::ComputedSignatureKey,
                    );
                    self.visit_this_parameter(method.this_param.as_deref());
                    self.visit_parameters(&method.params, callable_context);
                    if let Some(parameters) = &method.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &method.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
                TSSignature::TSCallSignatureDeclaration(call) => {
                    self.visit_this_parameter(call.this_param.as_deref());
                    self.visit_parameters(&call.params, callable_context);
                    if let Some(parameters) = &call.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &call.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
                TSSignature::TSIndexSignature(index) => {
                    self.visit_index_parameters(&index.parameters);
                    self.visit_type(&index.type_annotation.type_annotation, leaf_context)
                }
                TSSignature::TSConstructSignatureDeclaration(constructor) => {
                    self.visit_parameters(
                        &constructor.params,
                        StructuralDependencyContext::CarrierOnly,
                    );
                    if let Some(parameters) = &constructor.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &constructor.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
            }
        }
    }

    fn visit_type(&mut self, ts_type: &TSType<'_>, context: StructuralDependencyContext) {
        match ts_type {
            TSType::TSTypeReference(reference) => {
                self.record(
                    dependency_path_from_type_name(&reference.type_name),
                    context,
                );
                if let Some(arguments) = &reference.type_arguments {
                    for argument in &arguments.params {
                        self.visit_type(argument, context);
                    }
                }
            }
            TSType::TSUnionType(union) => {
                for member in &union.types {
                    self.visit_type(member, context);
                }
            }
            TSType::TSIntersectionType(intersection) => {
                for member in &intersection.types {
                    self.visit_type(member, context);
                }
            }
            TSType::TSTypeLiteral(literal) => self.visit_signatures(
                &literal.members,
                context == StructuralDependencyContext::CarrierOnly,
            ),
            TSType::TSArrayType(array) => self.visit_type(&array.element_type, context),
            TSType::TSTupleType(tuple) => {
                for element in &tuple.element_types {
                    let nested = match element {
                        TSTupleElement::TSOptionalType(optional) => Some(&optional.type_annotation),
                        TSTupleElement::TSRestType(rest) => Some(&rest.type_annotation),
                        TSTupleElement::TSNamedTupleMember(named) => {
                            named.element_type.as_ts_type()
                        }
                        _ => element.as_ts_type(),
                    };
                    if let Some(nested) = nested {
                        self.visit_type(nested, context);
                    }
                }
            }
            TSType::TSConditionalType(conditional) => {
                for nested in [
                    &conditional.check_type,
                    &conditional.extends_type,
                    &conditional.true_type,
                    &conditional.false_type,
                ] {
                    self.visit_type(nested, context);
                }
            }
            TSType::TSMappedType(mapped) => {
                self.visit_type(&mapped.constraint, context);
                if let Some(name_type) = &mapped.name_type {
                    self.visit_type(name_type, StructuralDependencyContext::CarrierOnly);
                }
                if let Some(annotation) = &mapped.type_annotation {
                    self.visit_type(annotation, context);
                }
            }
            TSType::TSIndexedAccessType(indexed) => {
                let indexed_context = match context {
                    StructuralDependencyContext::CarrierOnly => {
                        StructuralDependencyContext::CarrierOnly
                    }
                    _ => StructuralDependencyContext::Root,
                };
                self.visit_type(&indexed.object_type, indexed_context);
                self.visit_type(&indexed.index_type, indexed_context);
            }
            TSType::TSTypeOperatorType(operator) => {
                self.visit_type(&operator.type_annotation, context);
            }
            TSType::TSParenthesizedType(parenthesized) => {
                self.visit_type(&parenthesized.type_annotation, context);
            }
            TSType::TSTemplateLiteralType(template) => {
                for nested in &template.types {
                    self.visit_type(nested, context);
                }
            }
            TSType::TSFunctionType(function) => {
                let parameter_context = match context {
                    StructuralDependencyContext::CarrierOnly => {
                        StructuralDependencyContext::CarrierOnly
                    }
                    StructuralDependencyContext::LeafProperty => {
                        StructuralDependencyContext::LeafProperty
                    }
                    _ => StructuralDependencyContext::CallableParam,
                };
                self.visit_this_parameter(function.this_param.as_deref());
                self.visit_parameters(&function.params, parameter_context);
                if let Some(parameters) = &function.type_parameters {
                    self.visit_type_parameters_for_carrier(parameters);
                }
                self.visit_type(
                    &function.return_type.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            TSType::TSConstructorType(constructor) => {
                let parameter_context = match context {
                    StructuralDependencyContext::CarrierOnly => {
                        StructuralDependencyContext::CarrierOnly
                    }
                    StructuralDependencyContext::LeafProperty => {
                        StructuralDependencyContext::LeafProperty
                    }
                    _ => StructuralDependencyContext::CallableParam,
                };
                self.visit_parameters(&constructor.params, parameter_context);
                if let Some(parameters) = &constructor.type_parameters {
                    self.visit_type_parameters_for_carrier(parameters);
                }
                self.visit_type(
                    &constructor.return_type.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            TSType::TSTypeQuery(query) => {
                let query_context = match &query.expr_name {
                    TSTypeQueryExprName::IdentifierReference(_) => context,
                    _ => StructuralDependencyContext::CarrierOnly,
                };
                let query_path = dependency_path_from_query_name(&query.expr_name);
                if let Some(path) = &query_path {
                    self.value_queries.insert(path.clone());
                }
                self.record(query_path, query_context);
                if let TSTypeQueryExprName::TSImportType(import) = &query.expr_name {
                    if let Some(arguments) = &import.type_arguments {
                        for argument in &arguments.params {
                            self.visit_type(argument, StructuralDependencyContext::CarrierOnly);
                        }
                    }
                }
                if let Some(arguments) = &query.type_arguments {
                    for argument in &arguments.params {
                        self.visit_type(argument, StructuralDependencyContext::CarrierOnly);
                    }
                }
            }
            TSType::TSTypePredicate(predicate) => {
                if let Some(annotation) = &predicate.type_annotation {
                    self.visit_type(
                        &annotation.type_annotation,
                        StructuralDependencyContext::CarrierOnly,
                    );
                }
            }
            TSType::TSInferType(infer) => {
                if let Some(constraint) = &infer.type_parameter.constraint {
                    self.visit_type(constraint, StructuralDependencyContext::CarrierOnly);
                }
                if let Some(default) = &infer.type_parameter.default {
                    self.visit_type(default, StructuralDependencyContext::CarrierOnly);
                }
            }
            TSType::TSImportType(import) => {
                if let Some(arguments) = &import.type_arguments {
                    for argument in &arguments.params {
                        self.visit_type(argument, StructuralDependencyContext::CarrierOnly);
                    }
                }
            }
            TSType::JSDocNullableType(nullable) => {
                self.visit_type(
                    &nullable.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            TSType::JSDocNonNullableType(non_nullable) => {
                self.visit_type(
                    &non_nullable.type_annotation,
                    StructuralDependencyContext::CarrierOnly,
                );
            }
            _ => {}
        }
    }

    fn visit_interface(
        &mut self,
        members: &[TSSignature<'_>],
        heritage: &[TSInterfaceHeritage<'_>],
    ) {
        for base in heritage {
            self.record_expression_path(&base.expression, StructuralDependencyContext::Root);
            if let Some(arguments) = &base.type_arguments {
                for argument in &arguments.params {
                    self.visit_type(argument, StructuralDependencyContext::Root);
                }
            }
        }
        self.visit_signatures(members, false);
    }

    fn visit_class(&mut self, class: &Class<'_>) {
        if let Some(parameters) = &class.type_parameters {
            self.visit_type_parameters_for_carrier(parameters);
        }
        if let Some(base) = &class.super_class {
            let value_path = dependency_path_from_expression(base);
            if let Some(path) = &value_path {
                self.value_positions.insert(path.clone());
            } else {
                self.unsupported_value_positions
                    .insert(UnsupportedValuePositionKind::ClassHeritageExpression);
            }
            let legacy_context = match base {
                Expression::Identifier(_) => StructuralDependencyContext::Root,
                _ => StructuralDependencyContext::CarrierOnly,
            };
            self.record(value_path, legacy_context);
            if let Some(arguments) = &class.super_type_arguments {
                for argument in &arguments.params {
                    self.visit_type(argument, StructuralDependencyContext::Root);
                }
            }
        }
        for clause in &class.implements {
            self.record(
                dependency_path_from_type_name(&clause.expression),
                StructuralDependencyContext::Root,
            );
            if let Some(arguments) = &clause.type_arguments {
                for argument in &arguments.params {
                    self.visit_type(argument, StructuralDependencyContext::Root);
                }
            }
        }
        for member in &class.body.body {
            match member {
                ClassElement::PropertyDefinition(property) => {
                    self.visit_computed_key(
                        &property.key,
                        property.computed,
                        UnsupportedValuePositionKind::ComputedClassKey,
                    );
                    if let Some(annotation) = &property.type_annotation {
                        self.visit_type(
                            &annotation.type_annotation,
                            StructuralDependencyContext::LeafProperty,
                        );
                    }
                }
                ClassElement::MethodDefinition(method) => {
                    self.visit_computed_key(
                        &method.key,
                        method.computed,
                        UnsupportedValuePositionKind::ComputedClassKey,
                    );
                    self.visit_this_parameter(method.value.this_param.as_deref());
                    self.visit_parameters(
                        &method.value.params,
                        StructuralDependencyContext::CallableParam,
                    );
                    if let Some(parameters) = &method.value.type_parameters {
                        self.visit_type_parameters_for_carrier(parameters);
                    }
                    if let Some(return_type) = &method.value.return_type {
                        self.visit_type(
                            &return_type.type_annotation,
                            StructuralDependencyContext::CarrierOnly,
                        );
                    }
                }
                ClassElement::AccessorProperty(property) => {
                    self.visit_computed_key(
                        &property.key,
                        property.computed,
                        UnsupportedValuePositionKind::ComputedClassKey,
                    );
                    if let Some(annotation) = &property.type_annotation {
                        self.visit_type(
                            &annotation.type_annotation,
                            StructuralDependencyContext::LeafProperty,
                        );
                    }
                }
                ClassElement::TSIndexSignature(index) => {
                    self.visit_index_parameters(&index.parameters);
                    self.visit_type(
                        &index.type_annotation.type_annotation,
                        StructuralDependencyContext::LeafProperty,
                    );
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod dependency_path_tests {
    use std::collections::{BTreeMap, BTreeSet, HashSet};

    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    use super::{
        analyze_external_type_program, analyze_external_type_program_for_owner,
        analyze_external_type_program_with_owner_table, analyze_external_type_source,
        collect_statement_dependencies, AnalyzedExternalTypeSymbolKind, DeclDependencyNames,
        DeclarationPath, ExportTarget, ImportBindingForm, ImportedExportPath, RoutedImportTarget,
        SymbolSpace, SyntaxCapability, TypeDeclarationMergePolicy,
    };
    use verter_type_expr::{DeclKey, TopLevelOwnerId};

    fn collect(source: &str) -> Vec<(String, DeclDependencyNames)> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        assert!(!parsed.panicked, "fixture must parse: {source}");
        parsed
            .program
            .body
            .iter()
            .flat_map(|statement| {
                collect_statement_dependencies(statement, TopLevelOwnerId::ordinary_file())
            })
            .map(|(declaration, dependencies)| {
                let mut label = declaration.root.name.to_string();
                for member in declaration.member_path() {
                    label.push('.');
                    label.push_str(member);
                }
                (label, dependencies)
            })
            .collect()
    }

    fn path_names(
        paths: &BTreeSet<verter_type_expr::facts::TypeDependencyPathFact>,
    ) -> BTreeSet<String> {
        paths.iter().map(|path| path.legacy_dotted_name()).collect()
    }

    #[test]
    fn alias_preserves_legacy_contexts_and_adds_carrier_only_positions() {
        let dependencies = collect(
            "type Alias = Root<Arg> & { leaf: Leaf; f: (x: Param) => Ret; \
             make: new (x: CtorParam) => CtorRet; indexed: Obj[Key] };",
        )
        .pop()
        .unwrap()
        .1;

        assert_eq!(
            path_names(&dependencies.dependency_paths),
            BTreeSet::from_iter(
                ["Arg", "CtorParam", "Key", "Leaf", "Obj", "Param", "Root",].map(str::to_string)
            ),
        );
        assert_eq!(
            path_names(&dependencies.structural_dependency_paths),
            BTreeSet::from_iter(["Arg", "Key", "Obj", "Root"].map(str::to_string)),
        );
        assert_eq!(
            path_names(&dependencies.declaration_carrier_paths),
            BTreeSet::from_iter(
                [
                    "Arg",
                    "CtorParam",
                    "CtorRet",
                    "Key",
                    "Leaf",
                    "Obj",
                    "Param",
                    "Ret",
                    "Root",
                ]
                .map(str::to_string)
            ),
        );
    }

    #[test]
    fn carrier_only_context_survives_nested_signature_shapes() {
        let dependencies = collect(
            "interface I<T extends { bound: Bound }> { \
               m(): { p: Prop; f(x: Param): Ret }; \
               new (x: CtorParam): { q: CtorRet }; \
             }",
        )
        .pop()
        .unwrap()
        .1;
        let carrier_only = BTreeSet::from_iter(
            ["Bound", "Prop", "Param", "Ret", "CtorParam", "CtorRet"].map(str::to_string),
        );

        assert!(path_names(&dependencies.dependency_paths).is_disjoint(&carrier_only));
        assert!(path_names(&dependencies.structural_dependency_paths).is_disjoint(&carrier_only));
        assert_eq!(
            path_names(&dependencies.declaration_carrier_paths),
            carrier_only,
        );
    }

    #[test]
    fn class_preserves_legacy_context_matrix_and_qualified_paths() {
        let dependencies = collect(
            "class C<T extends Bound> extends Base<SuperArg> \
             implements NS.Shape<ImplArg> { \
               field: Field; method(x: Param): Ret; accessor value: Access; \
             }",
        )
        .pop()
        .unwrap()
        .1;

        assert_eq!(
            path_names(&dependencies.dependency_paths),
            BTreeSet::from_iter(
                ["Access", "Base", "Field", "ImplArg", "NS.Shape", "Param", "SuperArg",]
                    .map(str::to_string)
            ),
        );
        assert_eq!(
            path_names(&dependencies.structural_dependency_paths),
            BTreeSet::from_iter(
                ["Base", "ImplArg", "NS.Shape", "Param", "SuperArg"].map(str::to_string),
            ),
        );
        let carrier = path_names(&dependencies.declaration_carrier_paths);
        assert!(carrier.contains("Bound"));
        assert!(carrier.contains("Ret"));
    }

    #[test]
    fn qualified_query_and_heritage_are_carrier_only_without_legacy_drift() {
        let mut records = collect(
            "type T = F.Bar | typeof NS.Value.Inner | this; \
             interface I extends NS.Base<Arg> {}",
        );
        let (_, interface) = records.pop().unwrap();
        let (_, alias) = records.pop().unwrap();

        assert_eq!(
            path_names(&alias.dependency_paths),
            BTreeSet::from_iter(["F.Bar".to_string()]),
        );
        assert_eq!(
            path_names(&alias.declaration_carrier_paths),
            BTreeSet::from_iter(["F.Bar", "NS.Value.Inner"].map(str::to_string)),
        );
        assert_eq!(
            path_names(&interface.dependency_paths),
            BTreeSet::from_iter(["Arg".to_string()]),
        );
        assert_eq!(
            path_names(&interface.declaration_carrier_paths),
            BTreeSet::from_iter(["Arg", "NS.Base"].map(str::to_string)),
        );
    }

    #[test]
    fn owner_keys_preserve_merges_default_aliases_and_namespaces() {
        let mut by_owner: BTreeMap<String, DeclDependencyNames> = BTreeMap::new();
        for (owner, dependencies) in collect(
            "interface Merge { a: A } interface Merge { b: B } \
             export default interface Named { p: P } \
             namespace Ns { export interface Inner { q: Q }; \
                            export class C extends Base {} }",
        ) {
            let entry = by_owner.entry(owner).or_default();
            entry.dependency_paths.extend(dependencies.dependency_paths);
            entry
                .structural_dependency_paths
                .extend(dependencies.structural_dependency_paths);
            entry
                .declaration_carrier_paths
                .extend(dependencies.declaration_carrier_paths);
        }

        assert_eq!(
            path_names(&by_owner["Merge"].dependency_paths),
            BTreeSet::from_iter(["A", "B"].map(str::to_string)),
        );
        assert_eq!(
            path_names(&by_owner["Named"].dependency_paths),
            path_names(&by_owner["default"].dependency_paths),
        );
        assert_eq!(
            path_names(&by_owner["Ns.Inner"].dependency_paths),
            BTreeSet::from_iter(["Q".to_string()]),
        );
        assert_eq!(
            path_names(&by_owner["Ns.C"].dependency_paths),
            BTreeSet::from_iter(["Base".to_string()]),
        );
    }

    #[test]
    fn analyzer_unions_merged_contributor_paths() {
        let allocator = Allocator::default();
        let analysis = analyze_external_type_source(
            "interface Merge { a: A } interface Merge { b: B }",
            &allocator,
        );
        let merged = analysis
            .local_type_symbol(&declaration(TopLevelOwnerId::ordinary_file(), "Merge", &[]))
            .unwrap();

        assert_eq!(
            path_names(&merged.dependency_paths),
            BTreeSet::from_iter(["A", "B"].map(str::to_string)),
        );
    }

    #[test]
    fn declaration_carrier_covers_erased_signature_and_type_forms() {
        let dependencies = collect(
            "type Carrier<Input> = \
               ((...rest: Rest[]) => Returned) | \
               ((x: unknown) => x is Predicate) | \
               (Input extends infer U extends InferBound ? U : never) | \
               import('./m')<ImportArg> | typeof factory<QueryArg>;",
        )
        .pop()
        .unwrap()
        .1;
        let carrier = path_names(&dependencies.declaration_carrier_paths);

        for expected in [
            "Rest",
            "Returned",
            "Predicate",
            "InferBound",
            "ImportArg",
            "factory",
            "QueryArg",
        ] {
            assert!(
                carrier.contains(expected),
                "missing carrier path {expected}"
            );
        }
    }

    #[test]
    fn value_roles_distinguish_queries_supported_heritage_and_unroutable_heritage() {
        let records = collect(
            "type Query = typeof NS.Value; \
             class Supported extends NS.Base {} \
             class Unsupported extends mixin(Base) {}",
        )
        .into_iter()
        .collect::<BTreeMap<_, _>>();

        assert_eq!(
            path_names(&records["Query"].value_query_paths),
            BTreeSet::from_iter(["NS.Value".to_string()]),
        );
        assert_eq!(
            path_names(&records["Supported"].value_position_paths),
            BTreeSet::from_iter(["NS.Base".to_string()]),
        );
        assert!(records["Supported"].unsupported_value_positions.is_empty());
        assert!(records["Unsupported"]
            .unsupported_value_positions
            .contains(&super::UnsupportedValuePositionKind::ClassHeritageExpression));
    }

    #[test]
    fn mapped_remap_is_a_declaration_carrier_dependency() {
        let dependencies =
            collect("type Remapped<T> = { [K in keyof T as Rename<K>]: Value<T[K]> };")
                .pop()
                .unwrap()
                .1;

        let carrier = path_names(&dependencies.declaration_carrier_paths);
        assert!(carrier.contains("Rename"), "mapped `as` type was skipped");
        assert!(carrier.contains("Value"), "mapped value type was skipped");
        assert!(carrier.contains("T"), "mapped constraint was skipped");
        assert!(
            !path_names(&dependencies.dependency_paths).contains("Rename"),
            "carrier completion must not broaden the legacy full set",
        );
        assert!(
            !path_names(&dependencies.structural_dependency_paths).contains("Rename"),
            "carrier completion must not broaden the legacy structural set",
        );
    }

    #[test]
    fn computed_signature_and_class_keys_are_value_dependencies() {
        let records = collect(
            "declare const memberKey: unique symbol; \
             interface Surface { [memberKey]: Payload; [NS.key](): Returned; [makeKey()](): Never } \
             class Carrier { [memberKey]: Payload; [NS.key](): Returned {}; [makeKey()](): Never {} }",
        )
        .into_iter()
        .collect::<BTreeMap<_, _>>();

        assert_eq!(
            path_names(&records["Surface"].value_position_paths),
            BTreeSet::from_iter(["NS.key", "memberKey"].map(str::to_string)),
        );
        assert_eq!(
            path_names(&records["Carrier"].value_position_paths),
            BTreeSet::from_iter(["NS.key", "memberKey"].map(str::to_string)),
        );
        assert!(records["Surface"]
            .unsupported_value_positions
            .contains(&super::UnsupportedValuePositionKind::ComputedSignatureKey));
        assert!(records["Carrier"]
            .unsupported_value_positions
            .contains(&super::UnsupportedValuePositionKind::ComputedClassKey));
    }

    fn parse<'a>(allocator: &'a Allocator, source: &'a str) -> oxc_parser::ParserReturn<'a> {
        let parsed = Parser::new(allocator, source, SourceType::ts()).parse();
        assert!(!parsed.panicked, "fixture must parse: {source}");
        parsed
    }

    fn declaration(owner: TopLevelOwnerId, root: &str, members: &[&str]) -> DeclarationPath {
        DeclarationPath::new(DeclKey::new(owner, root), members.iter().copied())
    }

    fn dependency(segments: &[&str]) -> verter_type_expr::facts::TypeDependencyPathFact {
        verter_type_expr::facts::TypeDependencyPathFact::from_segments(segments.iter().copied())
            .unwrap()
    }

    #[test]
    fn owner_table_keeps_same_declarations_imports_and_exports_disjoint() {
        let allocator = Allocator::default();
        let parsed = parse(
            &allocator,
            "import { External as Local } from './shared'; \
             export interface Same { module: ModuleValue } \
             import { External as Local } from './shared'; \
             export interface Same { instance: InstanceValue }",
        );
        let module = TopLevelOwnerId::module(0);
        let instance = TopLevelOwnerId::instance(0);
        let analysis = analyze_external_type_program_with_owner_table(
            &parsed.program,
            &[module, module, instance, instance],
        )
        .unwrap();

        let module_same = declaration(module, "Same", &[]);
        let instance_same = declaration(instance, "Same", &[]);
        assert!(analysis.local_type_symbol(&module_same).is_some());
        assert!(analysis.local_type_symbol(&instance_same).is_some());
        assert!(analysis
            .import_binding(&DeclKey::new(module, "Local"))
            .is_some());
        assert!(analysis
            .import_binding(&DeclKey::new(instance, "Local"))
            .is_some());
        assert_eq!(
            analysis.export_target(&DeclKey::new(module, "Same")),
            Some(&ExportTarget::LocalDeclaration {
                declaration: module_same,
                capability: SyntaxCapability::TypeOnly,
            }),
        );
        assert_eq!(
            analysis.export_target(&DeclKey::new(instance, "Same")),
            Some(&ExportTarget::LocalDeclaration {
                declaration: instance_same,
                capability: SyntaxCapability::TypeOnly,
            }),
        );
    }

    #[test]
    fn same_owner_import_and_export_collisions_fail_closed() {
        let allocator = Allocator::default();
        let owner = TopLevelOwnerId::ordinary_file();
        let parsed = parse(
            &allocator,
            "import { A as Local } from './one'; \
             import { A as Local } from './two'; \
             interface A {} interface B {} \
             export { A as Public }; export { B as Public };",
        );
        let analysis = analyze_external_type_program(&parsed.program);

        let local = DeclKey::new(owner, "Local");
        let public = DeclKey::new(owner, "Public");
        assert!(analysis.import_binding(&local).is_none());
        assert!(analysis.is_ambiguous_import(&local));
        assert!(analysis.export_target(&public).is_none());
        assert!(analysis.is_ambiguous_export(&public));
    }

    #[test]
    fn import_routes_preserve_form_capability_and_structural_member_paths() {
        let allocator = Allocator::default();
        let owner = TopLevelOwnerId::ordinary_file();
        let parsed = parse(
            &allocator,
            "import type { Foo as Named } from './named'; \
             import DefaultValue from './default'; \
             import * as NS from './namespace';",
        );
        let analysis = analyze_external_type_program(&parsed.program);

        assert_eq!(
            analysis.resolve_import_dependency(owner, &dependency(&["Named", "Inner"])),
            Some(RoutedImportTarget {
                source: "./named".into(),
                form: ImportBindingForm::Named,
                capability: SyntaxCapability::TypeOnly,
                exported: ImportedExportPath::Symbol(dependency(&["Foo", "Inner"])),
            }),
        );
        assert_eq!(
            analysis.resolve_import_dependency(owner, &dependency(&["DefaultValue", "Inner"])),
            Some(RoutedImportTarget {
                source: "./default".into(),
                form: ImportBindingForm::Default,
                capability: SyntaxCapability::TypeAndValue,
                exported: ImportedExportPath::Symbol(dependency(&["default", "Inner"])),
            }),
        );
        assert_eq!(
            analysis.resolve_import_dependency(owner, &dependency(&["NS", "Value", "Inner"])),
            Some(RoutedImportTarget {
                source: "./namespace".into(),
                form: ImportBindingForm::Namespace,
                capability: SyntaxCapability::TypeAndValue,
                exported: ImportedExportPath::Symbol(dependency(&["Value", "Inner"])),
            }),
        );
        assert_eq!(
            analysis.resolve_import_dependency(owner, &dependency(&["NS"])),
            Some(RoutedImportTarget {
                source: "./namespace".into(),
                form: ImportBindingForm::Namespace,
                capability: SyntaxCapability::TypeAndValue,
                exported: ImportedExportPath::NamespaceRoot,
            }),
        );
    }

    #[test]
    fn namespace_class_and_sibling_routes_remain_structural() {
        let allocator = Allocator::default();
        let owner = TopLevelOwnerId::ordinary_file();
        let parsed = parse(
            &allocator,
            "namespace Ns { \
               export class C extends Base {} \
               export namespace Sibling { export interface T { value: Leaf } } \
               export interface Use { c: C; t: Sibling.T } \
             }",
        );
        let analysis = analyze_external_type_program(&parsed.program);
        let class = declaration(owner, "Ns", &["C"]);
        let sibling = declaration(owner, "Ns", &["Sibling", "T"]);
        let usage = declaration(owner, "Ns", &["Use"]);

        assert!(analysis.local_type_symbol(&class).is_some());
        assert!(analysis.local_type_symbol(&sibling).is_some());
        assert_eq!(
            analysis.local_symbol_dependency_paths(&usage),
            BTreeSet::from([class, sibling]),
        );
    }

    #[test]
    fn same_owner_merges_but_cross_owner_declarations_never_merge() {
        let allocator = Allocator::default();
        let parsed = parse(
            &allocator,
            "interface Merge { a: A } interface Merge { b: B } \
             interface Merge { c: C }",
        );
        let module = TopLevelOwnerId::module(0);
        let instance = TopLevelOwnerId::instance(0);
        let analysis = analyze_external_type_program_with_owner_table(
            &parsed.program,
            &[module, module, instance],
        )
        .unwrap();

        assert_eq!(
            path_names(
                &analysis
                    .local_type_symbol(&declaration(module, "Merge", &[]))
                    .unwrap()
                    .dependency_paths,
            ),
            BTreeSet::from_iter(["A", "B"].map(str::to_string)),
        );
        assert_eq!(
            path_names(
                &analysis
                    .local_type_symbol(&declaration(instance, "Merge", &[]))
                    .unwrap()
                    .dependency_paths,
            ),
            BTreeSet::from(["C".to_string()]),
        );
    }

    #[test]
    fn ordinary_program_entry_is_exactly_module_zero() {
        let allocator = Allocator::default();
        let parsed = parse(&allocator, "import { A } from './a'; interface B { a: A }");
        let ordinary = analyze_external_type_program(&parsed.program);
        let explicit =
            analyze_external_type_program_for_owner(&parsed.program, TopLevelOwnerId::module(0));
        let key = declaration(TopLevelOwnerId::module(0), "B", &[]);

        assert_eq!(
            ordinary.local_type_symbol(&key),
            explicit.local_type_symbol(&key)
        );
        assert_eq!(
            ordinary.import_binding(&DeclKey::new(TopLevelOwnerId::module(0), "A")),
            explicit.import_binding(&DeclKey::new(TopLevelOwnerId::module(0), "A")),
        );
    }

    #[test]
    fn owner_table_rejects_incomplete_statement_assignment() {
        let allocator = Allocator::default();
        let parsed = parse(&allocator, "interface A {} interface B {}");
        let error = analyze_external_type_program_with_owner_table(
            &parsed.program,
            &[TopLevelOwnerId::module(0)],
        )
        .unwrap_err();

        assert_eq!(error.statement_count(), 2);
        assert_eq!(error.owner_count(), 1);
    }

    #[test]
    fn declaration_and_route_keys_are_hashable_serializable_and_stably_ordered() {
        let module = declaration(TopLevelOwnerId::module(0), "Ns", &["C"]);
        let instance = declaration(TopLevelOwnerId::instance(0), "Ns", &["C"]);
        let mut hashes = HashSet::new();
        assert!(hashes.insert(module.clone()));
        assert!(hashes.insert(instance.clone()));

        let ordered = BTreeSet::from([instance.clone(), module.clone()]);
        assert_eq!(
            ordered.into_iter().collect::<Vec<_>>(),
            vec![module.clone(), instance]
        );

        let json = serde_json::to_string(&module).unwrap();
        let round_trip: DeclarationPath = serde_json::from_str(&json).unwrap();
        assert_eq!(round_trip, module);
    }

    #[test]
    fn legal_type_declaration_merges_are_order_independent_and_export_capable() {
        let owner = TopLevelOwnerId::ordinary_file();
        for source in [
            "export class Merge { a: A } export interface Merge { b: B }",
            "export interface Merge { b: B } export class Merge { a: A }",
        ] {
            let allocator = Allocator::default();
            let analysis = analyze_external_type_source(source, &allocator);
            let key = declaration(owner, "Merge", &[]);
            let symbol = analysis.local_type_symbol(&key).unwrap();

            assert_eq!(
                symbol.merge_policy,
                TypeDeclarationMergePolicy::ClassInterface
            );
            assert_eq!(
                symbol.primary_kind(),
                Some(AnalyzedExternalTypeSymbolKind::Class)
            );
            assert_eq!(symbol.contributors.len(), 2);
            assert_eq!(
                analysis.export_target(&DeclKey::new(owner, "Merge")),
                Some(&ExportTarget::LocalDeclaration {
                    declaration: key,
                    capability: SyntaxCapability::TypeAndValue,
                })
            );
        }

        let allocator = Allocator::default();
        let analysis = analyze_external_type_source(
            "export interface Merge { a: A } export interface Merge { b: B }",
            &allocator,
        );
        let symbol = analysis
            .local_type_symbol(&declaration(owner, "Merge", &[]))
            .unwrap();
        assert_eq!(symbol.merge_policy, TypeDeclarationMergePolicy::Interface);
        assert_eq!(
            path_names(&symbol.dependency_paths),
            BTreeSet::from_iter(["A", "B"].map(str::to_string))
        );
    }

    #[test]
    fn illegal_type_declaration_merges_fail_closed_in_both_orders() {
        let owner = TopLevelOwnerId::ordinary_file();
        for source in [
            "export type Merge = A; export interface Merge { b: B }",
            "export interface Merge { b: B } export type Merge = A;",
            "export class Merge {} export class Merge {}",
        ] {
            let allocator = Allocator::default();
            let analysis = analyze_external_type_source(source, &allocator);
            let path = declaration(owner, "Merge", &[]);
            let export = DeclKey::new(owner, "Merge");

            assert!(analysis.local_type_symbol(&path).is_none());
            assert!(analysis.is_ambiguous_local_type_symbol(&path));
            assert!(analysis.export_target(&export).is_none());
            assert!(analysis.is_ambiguous_export(&export));
        }
    }

    #[test]
    fn local_dependency_resolution_walks_namespace_ancestors_before_file_root() {
        let allocator = Allocator::default();
        let owner = TopLevelOwnerId::ordinary_file();
        let analysis = analyze_external_type_source(
            "import { RootSibling } from './external'; \
             interface FileRoot {} \
             namespace Ns { \
               export interface RootSibling {} \
               export namespace Parent { \
                 export interface ParentSibling {} \
                 export namespace Child { \
                   export interface Use { a: ParentSibling; b: RootSibling; c: FileRoot } \
                 } \
               } \
             }",
            &allocator,
        );
        let usage = declaration(owner, "Ns", &["Parent", "Child", "Use"]);

        assert_eq!(
            analysis.local_symbol_dependency_paths(&usage),
            BTreeSet::from([
                declaration(owner, "FileRoot", &[]),
                declaration(owner, "Ns", &["RootSibling"]),
                declaration(owner, "Ns", &["Parent", "ParentSibling"]),
            ])
        );
        assert!(analysis.required_import_bindings(&usage).is_empty());
    }

    #[test]
    fn raw_source_surfaces_use_structural_namespace_declaration_paths() {
        let allocator = Allocator::default();
        let owner = TopLevelOwnerId::ordinary_file();
        let analysis = analyze_external_type_source(
            "namespace Ns { export interface I { value: Value } export class C {} }",
            &allocator,
        );
        let interface = declaration(owner, "Ns", &["I"]);
        let class = declaration(owner, "Ns", &["C"]);

        assert_eq!(
            analysis
                .raw_source_surfaces_for(&interface, SymbolSpace::Type)
                .len(),
            1
        );
        assert_eq!(
            analysis
                .raw_source_surfaces_for(&class, SymbolSpace::Type)
                .len(),
            1
        );
        assert_eq!(
            analysis
                .raw_source_surfaces_for(&class, SymbolSpace::Value)
                .len(),
            1
        );
        assert!(analysis
            .raw_source_surfaces_for(&declaration(owner, "I", &[]), SymbolSpace::Type)
            .is_empty());
    }

    #[test]
    fn export_assignments_are_owner_scoped_and_ambiguous_per_owner() {
        let allocator = Allocator::default();
        let module = TopLevelOwnerId::module(0);
        let instance = TopLevelOwnerId::instance(0);
        let parsed = parse(&allocator, "export = ModuleValue; export = InstanceValue;");
        let analysis =
            analyze_external_type_program_with_owner_table(&parsed.program, &[module, instance])
                .unwrap();
        assert_eq!(
            analysis.export_assignment_target(module),
            Some(&DeclKey::new(module, "ModuleValue"))
        );
        assert_eq!(
            analysis.export_assignment_target(instance),
            Some(&DeclKey::new(instance, "InstanceValue"))
        );

        let parsed = parse(&allocator, "export = FirstValue; export = SecondValue;");
        let analysis = analyze_external_type_program_for_owner(&parsed.program, module);
        assert!(analysis.export_assignment_target(module).is_none());
        assert!(analysis.is_ambiguous_export_assignment(module));
    }

    #[test]
    fn this_and_index_parameter_annotations_are_carrier_only_dependencies() {
        let records = collect(
            "type Fn = (this: FnThis, value: Value) => Return; \
             interface I { \
               method(this: MethodThis, value: Value): Return; \
               (this: CallThis, value: Value): Return; \
               [key: IndexKey]: IndexValue; \
             } \
             class C { \
               method(this: ClassThis, value: Value): Return {} \
               [key: ClassIndexKey]: ClassIndexValue; \
             }",
        )
        .into_iter()
        .collect::<BTreeMap<_, _>>();

        let expected = BTreeSet::from_iter(
            [
                ("Fn", "FnThis"),
                ("I", "MethodThis"),
                ("I", "CallThis"),
                ("I", "IndexKey"),
                ("C", "ClassThis"),
                ("C", "ClassIndexKey"),
            ]
            .map(|(declaration, dependency)| (declaration.to_string(), dependency.to_string())),
        );
        for (declaration, dependency) in expected {
            let facts = &records[&declaration];
            assert!(
                path_names(&facts.declaration_carrier_paths).contains(&dependency),
                "{declaration} omitted carrier-only {dependency}"
            );
            assert!(!path_names(&facts.dependency_paths).contains(&dependency));
            assert!(!path_names(&facts.structural_dependency_paths).contains(&dependency));
        }
    }

    #[test]
    fn jsdoc_wrappers_recurse_as_carrier_only_dependencies() {
        let records =
            collect("type Nullable = ?LegacyNullable; type NonNullable = !LegacyNonNullable;")
                .into_iter()
                .collect::<BTreeMap<_, _>>();

        for (declaration, dependency) in [
            ("Nullable", "LegacyNullable"),
            ("NonNullable", "LegacyNonNullable"),
        ] {
            let facts = &records[declaration];
            assert!(path_names(&facts.declaration_carrier_paths).contains(dependency));
            assert!(!path_names(&facts.dependency_paths).contains(dependency));
            assert!(!path_names(&facts.structural_dependency_paths).contains(dependency));
        }
    }

    #[test]
    fn declaration_path_deserialization_rejects_empty_segments_without_panicking() {
        let owner = serde_json::to_string(&TopLevelOwnerId::ordinary_file()).unwrap();
        for json in [
            format!(r#"{{"root":{{"owner":{owner},"name":""}},"members":[]}}"#),
            format!(r#"{{"root":{{"owner":{owner},"name":"Root"}},"members":[""]}}"#),
            format!(r#"{{"root":{{"owner":{owner},"name":"Root"}},"members":["   "]}}"#),
        ] {
            let result =
                std::panic::catch_unwind(|| serde_json::from_str::<DeclarationPath>(&json));
            assert!(result.is_ok(), "invalid JSON must not panic: {json}");
            assert!(result.unwrap().is_err(), "invalid path accepted: {json}");
        }
    }
}
