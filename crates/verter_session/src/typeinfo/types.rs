#![deny(missing_docs)]
//! Public DTO types for the host typeinfo substrate.
//!
//! These types form the boundary between the host's internal symbol /
//! semantic-graph state and downstream consumers (the `@verter/typeinfo`
//! TS package, MCP tools, IDE integrations). They are designed to be:
//!
//! - **Immutable.** No interior mutability or back-references into host
//!   state. Consumers may stash them across requests.
//! - **`Send + Sync`.** Safe to ship across thread boundaries and into
//!   the FFI / WASM adapter.
//! - **Span-aware.** `SymbolEntry::span` is SFC-absolute per the
//!   project's `verter_span` invariants.

use std::sync::Arc;

use verter_type_expr::TypeExpr;

use crate::semantic_query::ProjectionMode;

// ---------------------------------------------------------------------------
// Query level
// ---------------------------------------------------------------------------

/// The *amount of meaning* a typeinfo query asks the host to compute for a
/// declaration — the typeinfo unification's query-identity axis (codex
/// BINDING `TypeInfoQueryLevel`).
///
/// This is **query identity, NOT an env-hash dimension** (R21 — the five env
/// hashes `parse_env_hash` / `resolve_env_hash` / `type_env_hash` /
/// `lib_env_hash` / `project_identity` stay split and unchanged). Two queries
/// for the same declaration at different levels are DIFFERENT queries that
/// produce DIFFERENT results and therefore must occupy DISTINCT cache slots —
/// exactly like [`ProjectionMode`] / [`crate::semantic_query::SurfaceProvenanceContext`]
/// are folded into the semantic query identity rather than into any env hash.
/// The level is threaded into the request structs ([`ShallowSurfaceRequest`],
/// [`VueMacroSurfaceRequest`]) and into the scratch / surface cache key where
/// the two levels diverge (most importantly a `.vue`'s PUBLIC component type
/// vs its FULL macro metadata), never into a workspace env hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeInfoQueryLevel {
    /// The declaration's PUBLIC type — for a `.vue` SFC this is the
    /// synthesized public component type (`$props` / `$emit` / `$slots` /
    /// expose surface) that a TS `import Foo from './Foo.vue'` site sees,
    /// resolved through typeinfo WITHOUT calling component-meta. For a plain
    /// TS declaration it is the same one-level shallow surface the
    /// [`FullMetadata`](Self::FullMetadata) level returns — the distinction
    /// only bites for `.vue` carriers, where the public type is the
    /// synthesized instance surface rather than the raw macro type argument.
    PublicType,
    /// The declaration's FULL resolved metadata — the span-rich one-level
    /// [`crate::typeinfo::surface::TypeInfoSurface`] (and, for a `.vue` macro,
    /// the normalized component-meta DTOs the
    /// [`crate::typeinfo::adapters::vue`] surface adapter produces). This is
    /// the level the macro-surface normalizers consume.
    FullMetadata,
}

impl TypeInfoQueryLevel {
    /// Stable discriminant byte folded into scratch / surface cache keys.
    /// Distinct per level so the two levels never collide on one cache slot.
    /// This is a QUERY-IDENTITY tag, not an env-hash input.
    #[must_use]
    pub const fn cache_tag(self) -> u8 {
        match self {
            TypeInfoQueryLevel::PublicType => 0,
            TypeInfoQueryLevel::FullMetadata => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Symbol inventory
// ---------------------------------------------------------------------------

/// One top-level symbol declared in a file.
///
/// Returned by [`crate::VerterHost::list_file_symbols`] — each entry
/// describes a single declaration captured by the file's shallow symbol
/// inventory ([`crate::resolver_core::shallow_file_state::ShallowFileState`]).
/// The entry is purely descriptive: it does not carry resolved type data
/// or generic-instantiation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolEntry {
    /// The local declaration name. For type-and-value class / enum
    /// declarations the same name surfaces twice with distinct
    /// [`SymbolKind`] entries.
    pub name: String,
    /// Discriminator naming the declaration class.
    pub kind: SymbolKind,
    /// SFC-absolute span sourced from the
    /// [`verter_semantic::analysis::types::LocalDeclarationEntry`] for
    /// this declaration, when one is present in the analysis snapshot.
    /// `None` when the script-analysis snapshot did not capture a span
    /// (e.g. ambient declarations synthesised at lower stages).
    pub span: Option<verter_span::Span>,
    /// `true` when the declaration is exported from the file. Imported
    /// symbols are NOT surfaced (the inventory describes declarations
    /// owned by this file).
    pub is_exported: bool,
}

/// Discriminator for a [`SymbolEntry`].
///
/// Mirrors the project's shallow-state taxonomy: `TypeAlias`,
/// `Interface`, and `Class` come from
/// [`verter_semantic::analysis::type_eval::TypeDeclKind`]; the value
/// kinds (`Const`, `Let`, `Var`, `Function`, `AsyncFunction`, `Enum`)
/// come from
/// [`verter_semantic::analysis::type_eval::ValueDeclKind`]. Class /
/// Enum surface twice in the inventory — once as a type entry and
/// once as a value entry — so consumers downstream of the inventory
/// can disambiguate the two namespaces without re-analysing the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    /// `type Foo = ...`
    TypeAlias,
    /// `interface Foo { ... }`
    Interface,
    /// `class Foo {}` — the type-side projection.
    Class,
    /// `const foo = ...`
    Const,
    /// `let foo`
    Let,
    /// `var foo`
    Var,
    /// `function foo() { ... }`
    Function,
    /// `async function foo() { ... }`
    AsyncFunction,
    /// `class Foo {}` — the value-side projection.
    ClassValue,
    /// `enum Foo {}` — appears with both a type and a value entry.
    Enum,
}

// ---------------------------------------------------------------------------
// Type-expression evaluation request
// ---------------------------------------------------------------------------

/// Request for [`crate::VerterHost::evaluate_type_expression_with_audit`].
///
/// Encodes the full inputs needed to synthesise a scratch TypeScript
/// file, evaluate one trailing `type __VerterScratch = <expression>`
/// declaration in the scope of `scope`, and return the resolved
/// semantic-graph node id.
///
/// The scratch URI is derived from a sha256 of `(scope_canonical,
/// expression, serialised(extra_imports))` — two scopes evaluating
/// the same expression always get distinct URIs.
#[derive(Debug, Clone)]
pub struct EvaluateTypeExpressionRequest {
    /// Canonical id of the file the expression evaluates against. The
    /// scratch file's import resolution and eval environment are
    /// rooted at this scope.
    pub scope: String,
    /// The TypeScript type expression body — a single type expression
    /// (`InstanceType<typeof default>['$props']`, `Pick<Foo, 'a'>`,
    /// `string`, …). Wrapped at synthesis time inside
    /// `type __VerterScratch = <expression>;`.
    pub expression: String,
    /// Additional imports to inject at the top of the scratch file.
    /// Each entry resolves through the host's normal import-route
    /// machinery so the expression can reference symbols not already
    /// in the source-scope.
    pub extra_imports: Vec<ImportSpec>,
    /// Projection mode for the terminal evaluation. `Identity` /
    /// `Navigate` / `Expanded` / `Shallow` follow the project's mode
    /// contract — see `/type-resolution` skill.
    pub mode: ProjectionMode,
    /// When `true`, the scratch file is published to the
    /// host-owned typeinfo scratch cache (default size 64) and a
    /// repeat request with the same URI hits the cache. When `false`,
    /// the synthesis is one-shot and the scratch file is dropped at
    /// the end of the call.
    pub cacheable: bool,
}

/// One structured import to inject into the scratch file synthesised
/// by [`EvaluateTypeExpressionRequest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSpec {
    /// Raw import specifier (e.g. `"./types"`, `"reka-ui"`).
    pub specifier: String,
    /// Per-binding shape. Multiple bindings on a single specifier
    /// surface as a single grouped `import` declaration in the
    /// synthesised source.
    pub bindings: Vec<NamedImport>,
}

/// One binding within an [`ImportSpec`].
///
/// `Default` corresponds to `import X from '...'`; `Named` to
/// `import { X } from '...'` / `import { X as Y } from '...'`;
/// `Namespace` to `import * as X from '...'`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedImport {
    /// `import <local_name> from "<specifier>"`
    Default {
        /// Local binding name.
        local_name: String,
    },
    /// `import { <exported_name> [as <local_alias>] } from "<specifier>"`
    /// — `type_only` controls whether the binding is rendered with
    /// the `type` qualifier.
    Named {
        /// Original exported name.
        exported_name: String,
        /// Optional local rename.
        local_alias: Option<String>,
        /// `true` for `import { type X }` / `import type { X }`.
        type_only: bool,
    },
    /// `import * as <local_name> from "<specifier>"`
    Namespace {
        /// Local namespace name.
        local_name: String,
    },
}

// ---------------------------------------------------------------------------
// Re-export aliases for the public host API
// ---------------------------------------------------------------------------

/// Type-arguments slice accepted by the public `resolve_named_symbol`
/// host methods. Aliased to a slice of [`TypeExpr`] for clarity at the
/// call site — the lowering happens inside the host method.
pub type TypeArgList<'a> = &'a [Arc<TypeExpr>];

// ---------------------------------------------------------------------------
// Level-aware surface requests
// ---------------------------------------------------------------------------

/// Request for the level-aware shallow-surface resolver
/// ([`crate::VerterHost::resolve_shallow_surface_for`]).
///
/// Threads the [`TypeInfoQueryLevel`] through the shallow-surface path so the
/// resolver does not grow a positional `level` argument on every call site.
/// `resolve_shallow_surface(canonical, name)` is the thin
/// [`TypeInfoQueryLevel::FullMetadata`] wrapper over this request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShallowSurfaceRequest {
    /// Canonical file id the declaration lives in.
    pub canonical_id: Arc<str>,
    /// The top-level declaration name to resolve.
    pub name: Arc<str>,
    /// The query level — query identity, NOT an env hash. For a plain TS
    /// declaration both levels return the same one-level surface; for a `.vue`
    /// carrier the levels diverge (PublicType = synthesized component type,
    /// FullMetadata = raw declaration surface). Enters the surface cache key
    /// via [`TypeInfoQueryLevel::cache_tag`].
    pub level: TypeInfoQueryLevel,
}

impl ShallowSurfaceRequest {
    /// Construct a request for `name` in `canonical_id` at `level`.
    #[must_use]
    pub fn new(canonical_id: Arc<str>, name: Arc<str>, level: TypeInfoQueryLevel) -> Self {
        Self {
            canonical_id,
            name,
            level,
        }
    }
}

/// Request for the typeinfo Vue-macro surface adapter
/// ([`crate::typeinfo::adapters::vue::resolve_vue_macro_surface`]).
///
/// Identifies ONE macro call inside a `.vue` SFC (`owner_canonical` +
/// `macro_index`) plus its kind and the query level. The adapter resolves the
/// macro's type-argument surface through the shared typeinfo surface path (the
/// same `resolve_shallow_surface` machinery, NEVER `surface_view_from_base_node`)
/// and returns the span-rich [`crate::typeinfo::adapters::vue::VueMacroSurface`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VueMacroSurfaceRequest {
    /// Canonical id of the `.vue` SFC that declares the macro.
    pub owner_canonical: Arc<str>,
    /// Stable index of the macro in the SFC's analysis snapshot
    /// (`FileAnalysisSnapshot::macros`).
    pub macro_index: usize,
    /// Which macro this request targets (`DefineProps` / `DefineEmits` /
    /// `DefineSlots` / `WithDefaults` / `DefineModel` / …).
    pub macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
    /// The `.vue` SFC's content identity (`IndexedReady::whole_hash`) — roots
    /// the surface to the content it was extracted from so a content edit
    /// produces a distinct cache identity. Carried explicitly so the adapter
    /// does not re-derive it per call.
    pub root_identity: verter_semantic::analysis::types::Hash16,
    /// The query level — query identity, NOT an env hash.
    pub level: TypeInfoQueryLevel,
}
