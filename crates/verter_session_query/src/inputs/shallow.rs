//! The shallow input record: a file's immutable syntax and routing facts, with no lowering
//! service or cache that could request source work.
use crate::analysis::route_inventory::ScriptRouteInventory;
use crate::declarations::header_index::TypeDeclHeader;
use crate::declarations::header_index::ValueDeclHeader;
use verter_type_expr::RouteDemand;

use crate::analysis::types::Hash16;
use crate::declarations::{TypeDeclKind, ValueDeclKind};
use crate::resolution::lowered_decl::{LoweredTypeDecl, LoweredValueDecl};
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use verter_span::Span;
use verter_type_expr::facts::TypeDependencyPathFact;
use verter_type_expr::{DeclBindingKey, TopLevelOwnerId, TypeAuthoredPropertyKey};

/// Presence lookups into the process-wide Svelte rune ambient inventory.
/// The inventory is built lazily by its owner on first lookup; a shallow
/// record of a rune module carries these so header-presence probes consult
/// the same inventory without naming its owner.
#[derive(Clone, Copy)]
pub struct RuneAmbientLookup {
    /// Whether the inventory declares a VALUE symbol of this name.
    pub has_value: fn(&str) -> bool,
    /// Whether the inventory declares a TYPE symbol of this name.
    pub has_type: fn(&str) -> bool,
}

impl std::fmt::Debug for RuneAmbientLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RuneAmbientLookup")
    }
}

/// Immutable syntax and routing facts. Contains no lowering service or cache
/// capable of requesting source work.
#[derive(Debug, Clone)]
pub struct ShallowInputRecord {
    pub whole_hash: Hash16,
    pub canonical_id: Arc<str>,
    pub source_identity: crate::source::snapshot::SnapshotKey,
    pub observation_id: u64,
    pub exports: FxHashMap<String, ExportTarget>,
    pub wildcard_reexports: Vec<WildcardReexport>,
    pub import_locals: FxHashSet<String>,
    pub import_targets: FxHashMap<String, ImportTarget>,
    pub owner_import_targets: FxHashMap<DeclBindingKey, ImportTarget>,
    pub route_inventory: Arc<ScriptRouteInventory>,
    pub headers: Arc<crate::declarations::header_index::DeclHeaderIndex>,
    pub owners: Arc<crate::analysis::top_level_owners::TopLevelOwnerTable>,
    pub synthesised_value_symbols: FxHashMap<DeclBindingKey, Arc<ShallowValueSymbol>>,
    pub export_assignment: Option<String>,
    /// The rune ambient inventory's presence lookups for a Svelte rune
    /// module; `None` for every other file.
    pub rune_ambient: Option<RuneAmbientLookup>,
}

/// A wildcard `export * from ‘...’` reexport — the AUTHORED specifier only.
///
/// Parse domain: the resolved target is NOT retained here. Resolution is
/// a resolve-domain answer owned by the workspace resolution authority
/// and demanded live by consumers; baking it into this content-addressed
/// artifact is what made the artifact go stale on an unrelated
/// dependency-set change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WildcardReexport {
    pub owner: TopLevelOwnerId,
    /// The raw source specifier (e.g., `./types`).
    pub source_specifier: String,
}

/// An import target — the AUTHORED specifier and imported name only.
///
/// Parse domain: no resolved canonical is retained (see
/// [`WildcardReexport`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportTarget {
    /// The raw source specifier (e.g., `./types`).
    pub source_specifier: String,
    /// The original exported name in the source module.
    pub imported_name: String,
    /// Whether the local binding is a namespace import (`import * as NS`)
    /// or an import assignment (`import NS = require("m")`).
    pub is_namespace: bool,
}

/// The [`ImportTarget::imported_name`] of an import assignment
/// (`import x = require("m")`) — TypeScript's own name for the value a
/// module assigns with `export =`, which the binding is when the module has
/// one (its namespace otherwise). No identifier can spell it.
pub const IMPORT_EQUALS_NAME: &str = "export=";

impl ImportTarget {
    /// Whether the binding is an import assignment
    /// (`import x = require("m")`).
    #[must_use]
    pub fn is_import_equals(&self) -> bool {
        self.is_namespace && self.imported_name == IMPORT_EQUALS_NAME
    }
}

/// Where an exported name resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportTarget {
    /// Locally declared and exported.
    Local {
        owner: TopLevelOwnerId,
        symbol_name: String,
    },
    /// Explicitly re-exported from another module.
    /// `export { Foo } from './bar'` or `export { Foo as Bar } from './bar'`
    Reexport {
        source_specifier: String,
        original_name: String,
        /// Whether this is a type-only reexport (`export type { ... }`).
        /// Used by the export graph to choose type vs. value resolution.
        is_type: bool,
    },
}

/// Slim HEADER metadata for one locally-declared type symbol.
///
/// This is a header-only view over the shallow declaration index — it
/// OWNS no body product. Declaration BODIES live exclusively in the
/// memo-owned [`LoweredTypeDecl`] (read through [`ShallowFileState::type_decl`]);
/// dependency edges live in [`ClassifiedTypeDeps`] (read through
/// [`ShallowFileState::type_deps`]).
#[derive(Debug, Clone)]
pub struct ShallowTypeSymbol {
    /// Declaration kind (header fact).
    pub kind: TypeDeclKind,
    /// Full declaration span of the last source-order contributor.
    pub span: Span,
    /// Generic type-parameter NAMES, unioned across contributors in
    /// first-seen order. (The full `TypeParam` carriers — constraints /
    /// defaults — are body data, read through `type_decl`.)
    pub type_param_names: Vec<String>,
    /// Direct syntactic member KEYS (own members only, heritage
    /// excluded) — a shallow shape fact.
    pub member_names: Vec<TypeAuthoredPropertyKey>,
    /// Number of same-name contributing declarations that merged into
    /// this symbol.
    pub contributor_count: usize,
}

impl ShallowTypeSymbol {
    /// Build the slim header view from a shallow type-declaration header.
    fn from_header(header: &TypeDeclHeader) -> Self {
        Self {
            kind: header.kind,
            span: header.span,
            type_param_names: header.type_params.iter().map(|p| p.name.clone()).collect(),
            member_names: header
                .member_headers
                .iter()
                .map(|m| m.key.clone())
                .collect(),
            contributor_count: header.contributors.len(),
        }
    }
}

/// Per-symbol dependency-edge classification — the local vs external
/// split over one type declaration's reference graph, baked against the
/// owning state's import targets. Dependency EDGES only; no body product.
#[derive(Debug, Clone, Default)]
pub struct ClassifiedTypeDeps {
    /// Names of same-file symbols this type directly depends on.
    /// Used for iterative local closure.
    pub local_deps: Vec<String>,
    /// Same-file runtime values reached through a type query (`typeof seed`).
    /// These are not type-closure hops: consumers that omit the owning body
    /// must provide a declaration-safe value carrier or reject the projection.
    pub owner_value_deps: Vec<String>,
    /// Same-file dual-space roots reached in a runtime-value role. Their
    /// exact declaration contributors can satisfy body-omitting output.
    pub retained_value_carrier_deps: Vec<String>,
    /// Names of import-local symbols this type directly depends on.
    /// These become `ExternalSymbolRef` during frontier traversal.
    pub external_deps: Vec<ExternalSymbolRef>,
    /// TSC declaration-carrier closure. Local names follow the validated
    /// lexical-owner chain (an instance owner may fall back to its unique
    /// module owner); the TSC projector resolves each name back to its exact
    /// owner before emitting a carrier. Kept separate so component-meta keeps
    /// its exact-owner FULL/STRUCTURAL breadth.
    pub declaration_local_deps: Vec<String>,
    pub declaration_external_deps: Vec<ExternalSymbolRef>,
    /// Bare namespace roots cannot identify an exported declaration carrier.
    pub unroutable_declaration_dependencies: Vec<String>,
    pub has_unroutable_value_position: bool,
    /// Import-local roots reached through `typeof` queries.
    pub external_value_queries: Vec<String>,
    /// Import-local roots required in a runtime value position by declaration
    /// syntax, currently class `extends` heritage.
    pub external_value_positions: Vec<String>,
}

/// Classification of an arbitrary parser-authored dependency-path set against
/// this file's import table and local header inventory.
#[derive(Debug, Clone, Default)]
pub struct ClassifiedDependencyPaths {
    pub local_deps: Vec<String>,
    pub external_deps: Vec<ExternalSymbolRef>,
    pub unroutable_imports: Vec<String>,
}

#[derive(Clone, Copy)]
pub enum LexicalValueBinding<'a> {
    Import(&'a ImportTarget),
    Local(TopLevelOwnerId),
}

/// Slim HEADER metadata for one locally-declared value symbol.
///
/// A header-only view over the shallow declaration index (kind +
/// object-literal member names) plus the `.vue`-default provenance flag.
/// It OWNS no body product — declaration bodies live exclusively in the
/// memo-owned (or eager synthesised) [`LoweredValueDecl`], read through
/// [`ShallowFileState::value_decl`].
#[derive(Debug, Clone)]
pub struct ShallowValueSymbol {
    /// Declaration kind (header fact).
    pub kind: ValueDeclKind,
    /// Direct member KEYS of an object-literal initializer
    /// (`const x = { a, b }`) — a shallow shape fact; empty for
    /// non-object-literal values.
    pub object_member_headers: Vec<TypeAuthoredPropertyKey>,
    /// Structural PROVENANCE fact: `true` only for the synthesized `default`
    /// VALUE symbol that [`super::vue_default_synth::synthesise_vue_default_value_symbol`]
    /// fabricates for a `.vue` SFC's implicit public instance (the construct
    /// signature returning `{ $props, $emit, $slots }`). `false` for EVERY
    /// userland-declared value symbol — including a userland `export default`
    /// in a `.vue`'s `<script>` block.
    ///
    /// This is the direct consumer proof that a resolved `default` IS the
    /// synthesized public instance. Synthesized-default consumers
    /// (`build_vue_default_instance`, the `.vue default` branch in
    /// `build_instantiate`, `resolve_vue_public_type`, the synthesized-default
    /// convergence in `build_typeof`) gate on this flag rather than on the
    /// file-classifier `is_synthesis_candidate`, so a `.vue` with a USERLAND
    /// `export default` (synthesis skipped, userland default present) is never
    /// mistreated as the synthesized public instance.
    pub is_synthesised_component_default: bool,
}

impl ShallowValueSymbol {
    /// Build the slim header view from a shallow value-declaration header.
    /// `is_synthesised_component_default` is `false` for every header-index
    /// (userland) value symbol.
    fn from_header(header: &ValueDeclHeader) -> Self {
        Self {
            kind: header.kind,
            object_member_headers: header
                .object_member_headers
                .iter()
                .map(|m| m.key.clone())
                .collect(),
            is_synthesised_component_default: false,
        }
    }

    /// Build the slim header view for the EAGER synthesised `.vue`-default
    /// from its macro-producer lowered body. The body itself is stored
    /// separately ([`ShallowFileState::synthesised_value_bodies`]); this
    /// is the header probe carrying the provenance flag.
    pub fn synthesised_from_lowered(lowered: &LoweredValueDecl) -> Self {
        Self {
            kind: lowered.kind,
            object_member_headers: Vec::new(),
            is_synthesised_component_default: true,
        }
    }
}

/// A reference to an imported symbol that needs cross-file resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ExternalSymbolRef {
    /// The local import name in this file.
    pub local_name: String,
    /// The import specifier (e.g., `./types`, `reka-ui`).
    pub source_specifier: String,
    /// The original exported name in the source module.
    pub imported_name: String,
    /// The remaining route demand on the imported symbol.
    pub route: RouteDemand,
}

impl ShallowInputRecord {
    /// Returns `true` when the shallow state carries no meaningful content
    /// (no type symbols, no value symbols, no exports, no wildcard reexports,
    /// and no import targets). A non-empty state is worth caching and
    /// returning to callers even when the symbol inventory alone is empty
    /// (e.g. a barrel file with only reexports, or an SFC with only value
    /// bindings). Header-level check — no body lowering.
    pub fn is_empty(&self) -> bool {
        !self.has_any_type_symbol_names()
            && !self.has_any_value_symbol_names()
            && self.exports.is_empty()
            && self.wildcard_reexports.is_empty()
            && self.import_targets.is_empty()
    }

    /// Returns `true` when this state has content that the frontier can
    /// actually resolve against: local type/value symbols, direct exports,
    /// or wildcard reexport entries. Files that only contain imports but no
    /// exports or symbols should not be handed to the frontier — they have
    /// nothing to contribute to export resolution. Header-level check.
    pub fn has_resolvable_surface(&self) -> bool {
        self.has_any_type_symbol_names()
            || self.has_any_value_symbol_names()
            || !self.exports.is_empty()
            || !self.wildcard_reexports.is_empty()
    }

    fn has_any_type_symbol_names(&self) -> bool {
        !self.headers.type_headers.is_empty()
    }

    fn has_any_value_symbol_names(&self) -> bool {
        !self.headers.value_headers.is_empty() || !self.synthesised_value_symbols.is_empty()
    }

    /// Exact declaration key recorded for an exported synthesized value.
    /// The route is the authority: this never searches another owner by name.
    pub fn synthesised_export_decl_key(&self, exported_name: &str) -> Option<DeclBindingKey> {
        let ExportTarget::Local { owner, symbol_name } = self.exports.get(exported_name)? else {
            return None;
        };
        let key = DeclBindingKey::new(*owner, symbol_name.as_str());
        self.synthesised_value_symbols
            .contains_key(&key)
            .then_some(key)
    }

    /// Exact top-level owner used by compatibility queries that cannot carry
    /// an owner coordinate themselves.
    ///
    /// A framework component's synthesized `default` export is the producer
    /// fact naming its semantic instance owner. Ordinary files (and component
    /// files without that synthesized route) retain the historical Module(0)
    /// owner. This chooses an owner before declaration lookup; it never scans
    /// same-name declarations in another owner.
    pub fn default_semantic_owner(&self) -> TopLevelOwnerId {
        self.synthesised_export_decl_key("default")
            .map_or_else(TopLevelOwnerId::ordinary_file, |key| key.owner)
    }

    /// Every file-scope TYPE symbol name in the shallow inventory
    /// (header-level — no body lowering).
    pub fn type_symbol_names(&self) -> impl Iterator<Item = &str> {
        self.headers
            .type_headers
            .keys()
            .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
            .map(|key| key.name.as_ref())
    }

    /// Every file-scope VALUE symbol name in the shallow inventory,
    /// including eager synthesised symbols (header-level).
    pub fn value_symbol_names(&self) -> impl Iterator<Item = &str> {
        let headers = &self.headers.value_headers;
        headers
            .keys()
            .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
            .map(|key| key.name.as_ref())
            .chain(
                self.synthesised_value_symbols
                    .keys()
                    .filter(move |key| !headers.contains_key(*key))
                    .map(|key| key.name.as_ref()),
            )
    }

    /// Header-level kind of a file-scope TYPE symbol (no body lowering).
    pub fn type_symbol_kind(&self, name: &str) -> Option<crate::declarations::TypeDeclKind> {
        self.type_symbol_kind_in(TopLevelOwnerId::ordinary_file(), name)
    }

    /// Header-level kind of an exact owner-qualified TYPE symbol.
    pub fn type_symbol_kind_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<crate::declarations::TypeDeclKind> {
        self.headers
            .type_header_in(owner, name)
            .map(|header| header.kind)
    }

    /// Header-level kind of a file-scope VALUE symbol (no body lowering;
    /// synthesised symbols answer from their eager record).
    pub fn value_symbol_kind(&self, name: &str) -> Option<crate::declarations::ValueDeclKind> {
        if let Some(synthesised) = self
            .synthesised_export_decl_key(name)
            .and_then(|key| self.synthesised_value_symbols.get(&key))
        {
            return Some(synthesised.kind);
        }
        self.headers.value_header(name).map(|header| header.kind)
    }

    /// Header-level direct syntactic member headers of a file-scope TYPE
    /// symbol (no body lowering).
    pub fn type_member_headers(
        &self,
        name: &str,
    ) -> Option<&[crate::declarations::header_index::MemberHeader]> {
        self.headers
            .type_header(name)
            .map(|header| header.member_headers.as_slice())
    }

    /// Every `enum` declaration name in the shallow inventory
    /// (header-level). An enum symbol is registered DUAL-SPACE — it carries
    /// both a type header (its projected-type union) and a value header (its
    /// `typeof` object), so it IS yielded by both
    /// [`Self::type_symbol_names`] and [`Self::value_symbol_names`]. This
    /// dedicated enum table is the separate authority for the member
    /// (variant) NAMES — the member-presence facts rail — which the
    /// type/value headers do not carry.
    pub fn enum_symbol_names(&self) -> impl Iterator<Item = &str> {
        self.headers
            .enum_headers
            .keys()
            .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
            .map(|key| key.name.as_ref())
    }

    /// Header-level ordered member (variant) names of an `enum`
    /// declaration, in source order. `None` when `name` is not an enum.
    pub fn enum_member_names(&self, name: &str) -> Option<&[String]> {
        self.headers
            .enum_headers
            .get(&DeclBindingKey::new(TopLevelOwnerId::ordinary_file(), name))
            .map(|header| header.member_names.as_slice())
    }

    /// Header-level type-parameter names of a file-scope TYPE symbol.
    pub fn type_param_names(&self, name: &str) -> Option<Vec<&str>> {
        self.headers
            .type_header(name)
            .map(|header| header.type_params.iter().map(|p| p.name.as_str()).collect())
    }

    /// Header-level type-parameter headers of a file-scope TYPE symbol
    /// (each carries the param name plus the source locators of its
    /// constraint / default clauses). No body lowering.
    pub fn type_param_headers(
        &self,
        name: &str,
    ) -> Option<&[crate::declarations::header_index::TypeParamHeader]> {
        self.headers
            .type_header(name)
            .map(|header| header.type_params.as_slice())
    }

    /// Number of source-order contributing top-level statements for a
    /// file-scope TYPE symbol (a same-name decl split / merge changes
    /// this). No body lowering.
    pub fn type_contributor_count(&self, name: &str) -> Option<usize> {
        self.headers
            .type_header(name)
            .map(|header| header.contributors.len())
    }

    /// Header-level direct syntactic member headers of an object-literal
    /// initializer (or class-static surface) bound to a file-scope VALUE
    /// symbol. No body lowering.
    pub fn value_object_member_headers(
        &self,
        name: &str,
    ) -> Option<&[crate::declarations::header_index::MemberHeader]> {
        self.headers
            .value_header(name)
            .map(|header| header.object_member_headers.as_slice())
    }

    /// Number of source-order contributing top-level statements for a
    /// file-scope VALUE symbol. No body lowering.
    pub fn value_contributor_count(&self, name: &str) -> Option<usize> {
        self.headers
            .value_header(name)
            .map(|header| header.contributors.len())
    }

    /// Whether `name` is a file-scope TYPE symbol (header-level).
    pub fn has_type_symbol(&self, name: &str) -> bool {
        self.has_type_symbol_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn has_type_symbol_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        self.headers.type_header_in(owner, name).is_some()
    }

    /// Whether `name` is a file-scope VALUE symbol (header-level,
    /// including synthesised symbols).
    pub fn has_value_symbol(&self, name: &str) -> bool {
        self.headers.value_header(name).is_some()
            || self.synthesised_export_decl_key(name).is_some()
    }

    pub fn has_value_symbol_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        self.headers.value_header_in(owner, name).is_some()
            || self
                .synthesised_value_symbols
                .contains_key(&DeclBindingKey::new(owner, name))
    }

    /// Canonical one-way lexical parent for a carrier instance owner.
    ///
    /// The relation is derived exclusively from the validated owner table. An
    /// instance sees a sole module owner; module/frontmatter owners have no
    /// parent, and multiple module owners are ambiguous and fail closed.
    pub fn validated_lexical_parent_owner(
        &self,
        owner: TopLevelOwnerId,
    ) -> Option<TopLevelOwnerId> {
        self.owners.validated_lexical_parent_owner(owner)
    }

    fn lexical_owner_chain(&self, owner: TopLevelOwnerId) -> impl Iterator<Item = TopLevelOwnerId> {
        std::iter::once(owner).chain(self.validated_lexical_parent_owner(owner))
    }

    pub fn visible_value_binding(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<LexicalValueBinding<'_>> {
        for candidate in self.lexical_owner_chain(owner) {
            if let Some(target) = self
                .owner_import_targets
                .get(&DeclBindingKey::new(candidate, name))
            {
                return Some(LexicalValueBinding::Import(target));
            }
            if self.effective_value_header_present_in(candidate, name) {
                return Some(LexicalValueBinding::Local(candidate));
            }
        }
        None
    }

    /// Exact declaration owner of the first visible local TYPE binding.
    ///
    /// Imports shadow parent declarations in the same lexical lookup, while
    /// instance-to-module visibility is admitted only by the validated
    /// one-way parent relation.
    pub fn visible_local_type_owner(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<TopLevelOwnerId> {
        for candidate in self.lexical_owner_chain(owner) {
            if self
                .owner_import_targets
                .contains_key(&DeclBindingKey::new(candidate, name))
            {
                return None;
            }
            if self.effective_type_header_present_in(candidate, name) {
                return Some(candidate);
            }
        }
        None
    }

    /// Every `(scope, name)` key in the augmentation-scope TYPE inventory
    /// (header-level).
    pub fn augmentation_type_keys(
        &self,
    ) -> impl Iterator<Item = (&crate::declarations::AugmentationScopeKind, &str)> {
        self.headers
            .augmentation_type_headers
            .iter()
            .flat_map(|(scope, names)| {
                names
                    .keys()
                    .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
                    .map(move |key| (scope, key.name.as_ref()))
            })
    }

    pub fn augmentation_type_decl_keys(
        &self,
    ) -> impl Iterator<Item = (&crate::declarations::AugmentationScopeKind, &DeclBindingKey)> {
        self.headers
            .augmentation_type_headers
            .iter()
            .flat_map(|(scope, declarations)| declarations.keys().map(move |key| (scope, key)))
    }

    /// Whether this file owns any type- or value-space ambient augmentation
    /// declarations. These declarations require a prepared bundle even when
    /// the ordinary file surface is empty, because the bundle owns their exact
    /// import canonicalization and dependency facts.
    pub fn has_augmentation_declarations(&self) -> bool {
        let headers = &self.headers;
        headers
            .augmentation_type_headers
            .values()
            .any(|declarations| !declarations.is_empty())
            || headers
                .augmentation_value_headers
                .values()
                .any(|declarations| !declarations.is_empty())
    }

    /// Every `(scope, name)` key in the augmentation-scope VALUE inventory
    /// (header-level).
    pub fn augmentation_value_keys(
        &self,
    ) -> impl Iterator<Item = (&crate::declarations::AugmentationScopeKind, &str)> {
        self.headers
            .augmentation_value_headers
            .iter()
            .flat_map(|(scope, names)| {
                names
                    .keys()
                    .filter(|key| key.owner == TopLevelOwnerId::ordinary_file())
                    .map(move |key| (scope, key.name.as_ref()))
            })
    }

    /// Look up a named export. Returns `None` if the name is not directly
    /// exported (may still be available through wildcard reexports).
    pub fn export_target(&self, name: &str) -> Option<&ExportTarget> {
        self.exports.get(name)
    }

    /// The local value name a CommonJS `export = X` assigns the whole module
    /// to (`Some("X")`), or `None` for an ordinary ESM module. Part of the
    /// shallow EXPORT inventory; consumed by `typeof import("./m")` resolution.
    pub fn export_assignment_target(&self) -> Option<&str> {
        self.export_assignment.as_deref()
    }

    /// Whether the value `export = X` assigns can be called or constructed
    /// (X is a function or class declaration): a namespace import of the
    /// module names X by `default` only then. `None` for a module without an
    /// export assignment.
    pub fn export_assignment_is_callable(&self) -> Option<bool> {
        use crate::declarations::ValueDeclKind;
        let assigned = self.export_assignment_target()?;
        Some(
            self.headers
                .value_header_in(verter_type_expr::TopLevelOwnerId::ordinary_file(), assigned)
                .is_some_and(|header| {
                    matches!(
                        header.kind,
                        ValueDeclKind::Function
                            | ValueDeclKind::AsyncFunction
                            | ValueDeclKind::Class
                    )
                }),
        )
    }

    /// Whether this file has any wildcard re-exports.
    pub fn has_wildcard_reexports(&self) -> bool {
        !self.wildcard_reexports.is_empty()
    }

    /// Whether the shallow PARSE inventory contains authored syntax that can
    /// demand a cross-file resolution: an import, wildcard or named reexport,
    /// or bindingless side-effect/empty-list import.
    ///
    /// This predicate classifies authored shape only. It carries no resolved
    /// canonical and is never a resolution-currency authority; consumers
    /// resolve each specifier through the request's captured resolution world.
    pub fn has_shallow_cross_file_edges(&self) -> bool {
        !self.import_targets.is_empty()
            || self.has_wildcard_reexports()
            || !self.route_inventory.bindingless_imports.is_empty()
            || self
                .exports
                .values()
                .any(|target| matches!(target, ExportTarget::Reexport { .. }))
    }

    /// Look up the slim HEADER view of a local TYPE symbol by name —
    /// header-only (no body lowering). A header miss returns `None`.
    pub fn symbol(&self, name: &str) -> Option<Arc<ShallowTypeSymbol>> {
        self.headers
            .type_header(name)
            .map(|header| Arc::new(ShallowTypeSymbol::from_header(header)))
    }

    /// Read the kind and span of an exact owner-qualified TYPE symbol without
    /// allocating a full [`ShallowTypeSymbol`] view.
    pub fn type_symbol_metadata_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<(TypeDeclKind, Span)> {
        self.headers
            .type_header_in(owner, name)
            .map(|header| (header.kind, header.span))
    }

    /// Look up the slim HEADER view of a local VALUE symbol by name —
    /// synthesised symbols first (eager macro-producer header records),
    /// then the header index. Header-only (no body lowering).
    pub fn value_symbol(&self, name: &str) -> Option<Arc<ShallowValueSymbol>> {
        if let Some(synthesised) = self
            .synthesised_export_decl_key(name)
            .and_then(|key| self.synthesised_value_symbols.get(&key))
        {
            return Some(Arc::clone(synthesised));
        }
        self.headers
            .value_header(name)
            .map(|header| Arc::new(ShallowValueSymbol::from_header(header)))
    }

    /// Exact-owner HEADER lookup for a local VALUE declaration. Synthesized
    /// component defaults participate only under the owner recorded in their
    /// local export route; a same-name slot under another owner is a miss.
    pub fn value_symbol_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<Arc<ShallowValueSymbol>> {
        if let Some(synthesised) = self
            .synthesised_value_symbols
            .get(&DeclBindingKey::new(owner, name))
        {
            return Some(Arc::clone(synthesised));
        }
        self.headers
            .value_header_in(owner, name)
            .map(|header| Arc::new(ShallowValueSymbol::from_header(header)))
    }

    /// VALUE-symbol PRESENCE under the centralized effective lookup — header
    /// presence first (no body materialisation), then the rune ambient
    /// inventory for a rune module. Mirrors [`Self::effective_value_decl`]'s
    /// precedence without lowering a body.
    pub fn effective_value_header_present(&self, name: &str) -> bool {
        self.effective_value_header_present_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn effective_value_header_present_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        if self.has_value_symbol_in(owner, name) {
            return true;
        }
        owner == TopLevelOwnerId::ordinary_file()
            && self
                .rune_ambient
                .is_some_and(|lookup| (lookup.has_value)(name))
    }

    /// TYPE-symbol PRESENCE under the centralized effective lookup.
    pub fn effective_type_header_present(&self, name: &str) -> bool {
        self.effective_type_header_present_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn effective_type_header_present_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        if self.has_type_symbol_in(owner, name) {
            return true;
        }
        owner == TopLevelOwnerId::ordinary_file()
            && self
                .rune_ambient
                .is_some_and(|lookup| (lookup.has_type)(name))
    }

    /// The ambient block whose value `(owner, name)` the file's identity
    /// `(canonical, owner, name)` names when the file surface declares no
    /// such value: a MODULE's own `declare global { … }` member, else the
    /// member of the one `declare module "…" { … }` block that declares it.
    /// `None` when neither does, or when several module blocks do (the
    /// identity cannot say which). A script's `declare global` binds
    /// nothing.
    pub fn value_fallback_augmentation_scope(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<crate::declarations::AugmentationScopeKind> {
        use crate::declarations::AugmentationScopeKind;
        let headers = &self.headers;
        if crate::inputs::contributors::classify_shallow_module_kind(self)
            == crate::inputs::contributors::FileModuleKind::Module
            && headers
                .augmentation_value_header_in(&AugmentationScopeKind::Global, owner, name)
                .is_some()
        {
            return Some(AugmentationScopeKind::Global);
        }
        headers
            .sole_module_augmentation_value_scope(owner, name)
            .cloned()
    }

    /// The ambient block whose type `(owner, name)` the file's identity
    /// `(canonical, owner, name)` names when the file surface declares no
    /// such type: the file's `declare global { … }` member (a global is
    /// visible from any scope), else the member of the one
    /// `declare module "…" { … }` block that declares it — what the block's
    /// own references to the name read. `None` when neither does, or when
    /// several module blocks do.
    pub fn type_fallback_augmentation_scope(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<crate::declarations::AugmentationScopeKind> {
        if self.has_global_augmentation(name) {
            return Some(crate::declarations::AugmentationScopeKind::Global);
        }
        self.headers
            .sole_module_augmentation_type_scope(owner, name)
            .cloned()
    }

    pub fn classify_dependency_paths(
        &self,
        declaration_owner: TopLevelOwnerId,
        declaration_name: &str,
        paths: &FxHashSet<TypeDependencyPathFact>,
    ) -> ClassifiedDependencyPaths {
        let mut local = FxHashSet::default();
        let mut external = FxHashSet::default();
        let mut unroutable = FxHashSet::default();

        for path in paths {
            let root = path.root();
            if let Some(target) = self
                .owner_import_targets
                .get(&DeclBindingKey::new(declaration_owner, root))
            {
                let (imported_name, member_path) = if target.is_namespace {
                    let Some((exported_name, member_path)) = path.member_path().split_first()
                    else {
                        unroutable.insert(root.to_string());
                        continue;
                    };
                    (exported_name.clone(), member_path)
                } else {
                    (target.imported_name.clone(), path.member_path())
                };
                let route = if member_path.is_empty() {
                    RouteDemand::Whole
                } else {
                    RouteDemand::member_path(member_path.iter().map(String::as_str))
                };
                external.insert(ExternalSymbolRef {
                    local_name: root.to_string(),
                    source_specifier: target.source_specifier.clone(),
                    imported_name,
                    route,
                });
                continue;
            }

            if root != declaration_name && self.has_type_symbol_in(declaration_owner, root) {
                local.insert(root.to_string());
            }
        }

        let mut local = local.into_iter().collect::<Vec<_>>();
        local.sort();
        let mut external = external.into_iter().collect::<Vec<_>>();
        external.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
                .then_with(|| {
                    let left_path: &[verter_type_expr::facts::FactPropertyKey] = match &left.route {
                        RouteDemand::MemberPath(path) => path,
                        _ => &[],
                    };
                    let right_path: &[verter_type_expr::facts::FactPropertyKey] = match &right.route
                    {
                        RouteDemand::MemberPath(path) => path,
                        _ => &[],
                    };
                    left_path.cmp(right_path)
                })
        });
        let mut unroutable = unroutable.into_iter().collect::<Vec<_>>();
        unroutable.sort();
        ClassifiedDependencyPaths {
            local_deps: local,
            external_deps: external,
            unroutable_imports: unroutable,
        }
    }

    /// Declaration-output dependency classification follows the same
    /// validated one-way lexical-owner chain as bare-name resolution. The
    /// returned local names are intentionally resolved to exact owners by the
    /// TSC projector, while imported carriers retain their structural route.
    fn classify_declaration_dependency_paths(
        &self,
        declaration_owner: TopLevelOwnerId,
        declaration_name: &str,
        paths: &FxHashSet<TypeDependencyPathFact>,
    ) -> ClassifiedDependencyPaths {
        let mut local = FxHashSet::default();
        let mut external = FxHashSet::default();
        let mut unroutable = FxHashSet::default();

        for path in paths {
            let root = path.root();
            for binding_owner in self.lexical_owner_chain(declaration_owner) {
                if let Some(target) = self
                    .owner_import_targets
                    .get(&DeclBindingKey::new(binding_owner, root))
                {
                    let (imported_name, member_path) = if target.is_namespace {
                        let Some((exported_name, member_path)) = path.member_path().split_first()
                        else {
                            unroutable.insert(root.to_string());
                            break;
                        };
                        (exported_name.clone(), member_path)
                    } else {
                        (target.imported_name.clone(), path.member_path())
                    };
                    let route = if member_path.is_empty() {
                        RouteDemand::Whole
                    } else {
                        RouteDemand::member_path(member_path.iter().map(String::as_str))
                    };
                    external.insert(ExternalSymbolRef {
                        local_name: root.to_string(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name,
                        route,
                    });
                    break;
                }

                if binding_owner == declaration_owner && root == declaration_name {
                    break;
                }
                if self.has_type_symbol_in(binding_owner, root) {
                    local.insert(root.to_string());
                    break;
                }
            }
        }

        let mut local = local.into_iter().collect::<Vec<_>>();
        local.sort();
        let mut external = external.into_iter().collect::<Vec<_>>();
        external.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
                .then_with(|| {
                    let left_path: &[verter_type_expr::facts::FactPropertyKey] = match &left.route {
                        RouteDemand::MemberPath(path) => path,
                        _ => &[],
                    };
                    let right_path: &[verter_type_expr::facts::FactPropertyKey] = match &right.route
                    {
                        RouteDemand::MemberPath(path) => path,
                        _ => &[],
                    };
                    left_path.cmp(right_path)
                })
        });
        let mut unroutable = unroutable.into_iter().collect::<Vec<_>>();
        unroutable.sort();
        ClassifiedDependencyPaths {
            local_deps: local,
            external_deps: external,
            unroutable_imports: unroutable,
        }
    }

    /// Classify an ALREADY-LOWERED declaration body's dependency edges. The
    /// shared classification core behind [`Self::classify_type_deps_in`]
    /// (file-scope symbols) AND the augmentation-scope prepare path: a
    /// `declare global` / `declare module` contributor body references the
    /// SAME import namespace as the containing file, so its external deps
    /// classify identically — an unresolvable referenced import must fail
    /// preparation with `MissingExternalOwner` instead of silently preparing
    /// a Complete surface.
    pub fn classify_lowered_type_deps(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
        lowered: &LoweredTypeDecl,
    ) -> Arc<ClassifiedTypeDeps> {
        let legacy = self.classify_dependency_paths(owner, name, &lowered.dependency_paths);
        let declaration = self.classify_declaration_dependency_paths(
            owner,
            name,
            &lowered.declaration_carrier_paths,
        );
        let local_deps = legacy.local_deps;
        let mut external_deps = legacy.external_deps;
        let declaration_local_deps = declaration.local_deps;
        let mut declaration_external_deps = declaration.external_deps;
        let mut unroutable_declaration_dependencies = declaration.unroutable_imports;

        let value_paths = lowered
            .value_query_paths
            .iter()
            .chain(lowered.value_position_paths.iter());
        let mut owner_value_deps = value_paths
            .clone()
            .filter_map(|path| {
                let root = path.root();
                let LexicalValueBinding::Local(value_owner) =
                    self.visible_value_binding(owner, root)?
                else {
                    return None;
                };
                (root != name && !self.has_type_symbol_in(value_owner, root))
                    .then(|| root.to_owned())
            })
            .collect::<Vec<_>>();
        owner_value_deps.sort();
        owner_value_deps.dedup();

        let mut retained_value_carrier_deps = value_paths
            .clone()
            .filter_map(|path| {
                let root = path.root();
                let LexicalValueBinding::Local(value_owner) =
                    self.visible_value_binding(owner, root)?
                else {
                    return None;
                };
                self.has_type_symbol_in(value_owner, root)
                    .then(|| root.to_owned())
            })
            .collect::<Vec<_>>();
        retained_value_carrier_deps.sort();
        retained_value_carrier_deps.dedup();

        // Value-role roots retain the import declaration even when they do not
        // identify a type-space exported symbol (notably bare namespace
        // queries). They are appended to both legacy and declaration rails;
        // the role vectors below tell TSC whether the import must be usable as
        // a runtime value.
        for path in lowered
            .value_query_paths
            .iter()
            .chain(lowered.value_position_paths.iter())
        {
            let root = path.root();
            if let Some(target) = self
                .owner_import_targets
                .get(&DeclBindingKey::new(owner, root))
            {
                let external = ExternalSymbolRef {
                    local_name: root.to_string(),
                    source_specifier: target.source_specifier.clone(),
                    imported_name: target.imported_name.clone(),
                    route: RouteDemand::Whole,
                };
                if !external_deps
                    .iter()
                    .any(|dependency| dependency.local_name == root)
                {
                    external_deps.push(external);
                }
            }
            if let Some(LexicalValueBinding::Import(target)) =
                self.visible_value_binding(owner, root)
            {
                if !declaration_external_deps
                    .iter()
                    .any(|dependency| dependency.local_name == root)
                {
                    declaration_external_deps.push(ExternalSymbolRef {
                        local_name: root.to_string(),
                        source_specifier: target.source_specifier.clone(),
                        imported_name: target.imported_name.clone(),
                        route: RouteDemand::Whole,
                    });
                }
            }
        }

        unroutable_declaration_dependencies.retain(|root| {
            !lowered
                .value_query_paths
                .iter()
                .chain(lowered.value_position_paths.iter())
                .any(|path| path.root() == root)
        });

        external_deps.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
        });
        declaration_external_deps.sort_by(|left, right| {
            left.local_name
                .cmp(&right.local_name)
                .then_with(|| left.source_specifier.cmp(&right.source_specifier))
                .then_with(|| left.imported_name.cmp(&right.imported_name))
        });
        let mut external_value_queries = lowered
            .value_query_paths
            .iter()
            .map(TypeDependencyPathFact::root)
            .filter(|root| {
                matches!(
                    self.visible_value_binding(owner, root),
                    Some(LexicalValueBinding::Import(_))
                )
            })
            .map(str::to_string)
            .collect::<FxHashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        external_value_queries.sort();
        let mut external_value_positions = lowered
            .value_position_paths
            .iter()
            .map(TypeDependencyPathFact::root)
            .filter(|root| {
                matches!(
                    self.visible_value_binding(owner, root),
                    Some(LexicalValueBinding::Import(_))
                )
            })
            .map(str::to_string)
            .collect::<FxHashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        external_value_positions.sort();

        Arc::new(ClassifiedTypeDeps {
            local_deps,
            owner_value_deps,
            retained_value_carrier_deps,
            external_deps,
            declaration_local_deps,
            declaration_external_deps,
            unroutable_declaration_dependencies,
            has_unroutable_value_position: lowered.has_unroutable_value_position,
            external_value_queries,
            external_value_positions,
        })
    }

    /// Whether this file declares any global (`declare global`) augmentation
    /// contributors for `name` (header-level check).
    pub fn has_global_augmentation(&self, name: &str) -> bool {
        self.headers
            .augmentation_type_header(&crate::declarations::AugmentationScopeKind::Global, name)
            .is_some()
    }

    /// Check if a name is an import-local binding.
    pub fn is_import_local(&self, name: &str) -> bool {
        self.is_import_local_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn is_import_local_in(&self, owner: TopLevelOwnerId, name: &str) -> bool {
        self.owner_import_targets
            .contains_key(&DeclBindingKey::new(owner, name))
    }

    /// Get the import target for a local import name.
    pub fn import_target(&self, local_name: &str) -> Option<&ImportTarget> {
        self.import_target_in(TopLevelOwnerId::ordinary_file(), local_name)
    }

    pub fn import_target_in(
        &self,
        owner: TopLevelOwnerId,
        local_name: &str,
    ) -> Option<&ImportTarget> {
        self.owner_import_targets
            .get(&DeclBindingKey::new(owner, local_name))
    }

    /// Every non-namespace local import binding in `owner` importing
    /// exactly `imported_name`, as `(local_name, source_specifier)`.
    ///
    /// PARSE DOMAIN only. The reverse lookup "which local alias names
    /// this resolved target?" needs a resolved canonical, which this
    /// artifact no longer retains; the caller resolves each returned
    /// specifier through the workspace resolution authority and applies
    /// the fail-closed uniqueness rule itself (an absent target, a
    /// namespace import, or two distinct locals resolving to the same
    /// target must yield no alias — a local spelling is never recovered
    /// by scanning another owner or by matching only the exported symbol
    /// name).
    pub fn local_imports_of_name_in<'a>(
        &'a self,
        owner: TopLevelOwnerId,
        imported_name: &'a str,
    ) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
        self.owner_import_targets
            .iter()
            .filter(move |(local, target)| {
                local.owner == owner
                    && !target.is_namespace
                    && target.imported_name == imported_name
            })
            .map(|(local, target)| (local.name.as_ref(), target.source_specifier.as_str()))
    }
}
