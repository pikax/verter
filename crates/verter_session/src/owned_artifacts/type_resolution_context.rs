//! `OwnedTypeResolutionContext` — owned, `Send + Sync + 'static` mirror
//! of `verter_parser::utils::oxc::vue::script::resolve_type::TypeResolutionContext`.
//!
//! ## Tier 1A authority (D18 + D45 + D65)
//!
//! Field-by-field migration from the borrowed
//! `TypeResolutionContext<'ctx, 'a>`:
//!
//! - `source: &'ctx [u8]` — DROPPED entirely (D65). All identifier
//!   comparisons go through `InternedIdentifierId`. The post-lowering
//!   pipeline has NO source-reread path; every byte the lowering saw is
//!   already encoded in the interned tables on `OwnedEvalProgram`.
//! - `type_aliases`, `interfaces`, `classes`, `type_params`,
//!   `type_param_bindings` — OWNED `FxHashMap<InternedIdentifierId, …>` /
//!   `Vec<…>` shapes.
//! - `type_param_bindings_cache_key`, `diagnostics`, `companion_types`,
//!   `companion_origins` — preserved (already owned in the borrowed
//!   form).
//! - `blocked_types`, `current_surface`, `companion_cache_key` —
//!   transient lowering-only state, NOT carried into
//!   `OwnedTypeResolutionContext`. The borrowed `TypeResolutionContext`
//!   stays around for the lowering call (D45) and these fields live
//!   there.
//! - `named_type_cache` — DROPPED. Its replacement is the
//!   `SemanticGraphStore::HostResolvedNamedTypeKey` identity map, which
//!   is the single named-type cache going forward.
//!
//! The `decl_arena: TypeDeclArena` and `span_arena: SpanArena` payloads
//! own the type-expression bodies referenced by `TypeAliasDeclId` /
//! `TypeParameterDeclId` / `TypeExprId`. The
//! `declaration_fingerprints` table (built at lowering) is consumed by
//! Tier 1B's TypeHandle resolution (D104).

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::eval_program::{InternedIdentifierId, SpanId};

#[cfg(test)]
#[path = "type_resolution_context_tests.rs"]
mod type_resolution_context_tests;

// ─────────────────────────────────────────────────────────────────────
// Compact id types for the owned arenas
// ─────────────────────────────────────────────────────────────────────

/// Index into [`TypeDeclArena::aliases`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeAliasDeclId(pub u32);

/// Index into [`TypeDeclArena::interfaces`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InterfaceDeclId(pub u32);

/// Index into [`TypeDeclArena::classes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClassDeclId(pub u32);

/// Index into [`TypeDeclArena::params`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeParameterDeclId(pub u32);

/// Index into [`TypeDeclArena::exprs`] — owned post-lowering `TypeExpr`
/// payload distinct from `TypeAliasDeclId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeExprId(pub u32);

/// Stable identity over [`DeclId`] for D104 TypeHandle resolution.
/// Computed via `blake3(canonical_id || content_hash || decl_name_bytes
/// || scope_path_bytes || decl_kind_byte)` — Tier 1B consumes this map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeclarationFingerprint(pub [u8; 16]);

impl DeclarationFingerprint {
    /// Construct from raw 16 bytes. The hashing schema is owned by
    /// Tier 1B (D104).
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}

/// Discriminated union over the owned decl-id space — the value side of
/// `declaration_fingerprints`. Tier 1B's `MetaSession::get_component_meta_type_expansion`
/// resolves a [`DeclarationFingerprint`] to a `DeclId` and walks the
/// corresponding arena entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeclId {
    Alias(TypeAliasDeclId),
    Interface(InterfaceDeclId),
    Class(ClassDeclId),
    Parameter(TypeParameterDeclId),
}

// ─────────────────────────────────────────────────────────────────────
// Owned arena payloads
// ─────────────────────────────────────────────────────────────────────

/// Owned mirror of the borrowed `TSType<'a>` AST node. Tier 1A
/// introduces the shell type; Tier 1C-α populates the variants from the
/// real lowering. The `Send + Sync + 'static` bound holds because every
/// payload is owned (no `&'a TSType<'a>` escapes into this struct).
#[derive(Debug, Clone)]
pub enum OwnedTypeExpr {
    /// Named reference (e.g., `Foo` or `Foo<T>`).
    Named {
        name: InternedIdentifierId,
        type_args: Vec<TypeExprId>,
    },
    /// Inline object type literal.
    Object {
        members: Vec<OwnedObjectMember>,
    },
    /// Union or intersection (discriminated by `kind`).
    Composite {
        kind: CompositeKind,
        arms: Vec<TypeExprId>,
    },
    /// Conditional type `T extends U ? X : Y`.
    Conditional {
        check: TypeExprId,
        extends: TypeExprId,
        true_branch: TypeExprId,
        false_branch: TypeExprId,
    },
    /// Mapped type.
    Mapped {
        key_param: TypeParameterDeclId,
        value: TypeExprId,
    },
    /// Indexed access `T[K]`.
    Indexed {
        target: TypeExprId,
        index: TypeExprId,
    },
    /// `keyof T`, `readonly T`, `unique symbol`, etc.
    TypeOperator {
        operator: TypeOperatorKind,
        operand: TypeExprId,
    },
    /// Tuple `[T, U]` — preserves element kinds (rest, optional, named).
    Tuple {
        elements: Vec<OwnedTupleElement>,
    },
    /// Array `T[]` (sugar over `Array<T>`).
    Array {
        element: TypeExprId,
    },
    /// Literal type (`"foo"`, `42`, `true`).
    Literal(super::eval_program::InternedLiteralId),
    /// `infer X` placeholder — only meaningful within a Conditional.
    Infer {
        target: InternedIdentifierId,
    },
    /// Unbound shell when lowering hit a known-unsupported shape that
    /// is *non*-macro-impacting (a regression on a macro-impacting
    /// shape would have produced `LoweringError`, not this).
    Unsupported {
        span: SpanId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompositeKind {
    Union,
    Intersection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeOperatorKind {
    Keyof,
    Readonly,
    Unique,
}

#[derive(Debug, Clone)]
pub struct OwnedObjectMember {
    pub name: InternedIdentifierId,
    pub value: TypeExprId,
    pub optional: bool,
    pub readonly: bool,
}

#[derive(Debug, Clone)]
pub struct OwnedTupleElement {
    pub value: TypeExprId,
    pub label: Option<InternedIdentifierId>,
    pub optional: bool,
    pub rest: bool,
}

/// Owned interface entry. Replaces the borrowed
/// `InterfaceResolutionEntry<'ctx, 'a>` shape; preserves the heritage
/// (extends) chain plus the inline member set.
#[derive(Debug, Clone)]
pub struct OwnedInterfaceEntry {
    pub name: InternedIdentifierId,
    pub heritage: Vec<TypeExprId>,
    pub members: Vec<OwnedObjectMember>,
    pub type_params: Vec<TypeParameterDeclId>,
    pub span: SpanId,
}

/// Owned class entry. Classes resolve to their instance-side shape in
/// type position.
#[derive(Debug, Clone)]
pub struct OwnedClassDecl {
    pub name: InternedIdentifierId,
    pub heritage: Vec<TypeExprId>,
    pub instance_members: Vec<OwnedObjectMember>,
    pub static_members: Vec<OwnedObjectMember>,
    pub type_params: Vec<TypeParameterDeclId>,
    pub span: SpanId,
}

/// Owned alias entry payload referenced via [`TypeAliasDeclId`].
#[derive(Debug, Clone)]
pub struct OwnedAliasEntry {
    pub name: InternedIdentifierId,
    pub body: TypeExprId,
    pub type_params: Vec<TypeParameterDeclId>,
    pub span: SpanId,
}

/// Owned type-parameter entry: `T extends U = V`.
#[derive(Debug, Clone)]
pub struct OwnedTypeParameter {
    pub name: InternedIdentifierId,
    pub constraint: Option<TypeExprId>,
    pub default: Option<TypeExprId>,
    pub span: SpanId,
}

/// Pre-lowered cache key for instantiation bindings — preserves the
/// borrowed-form's `Arc<[ResolvedTypeParamBindingCacheKey]>` shape but
/// uses owned ids so the slice can sit in a `Send + Sync` cache.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResolvedTypeParamBindingCacheKey {
    pub param: TypeParameterDeclId,
    pub bound: TypeExprId,
}

/// Resolution-time diagnostic. Mirror of the borrowed
/// `ResolutionDiagnostic` minus the AST-borrowed pointers.
#[derive(Debug, Clone)]
pub struct ResolutionDiagnostic {
    pub message: Arc<str>,
    pub span: SpanId,
    pub kind: ResolutionDiagnosticKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResolutionDiagnosticKind {
    UnknownType,
    Cycle,
    BudgetExceeded,
    Other,
}

/// Companion-type entry built from a sibling `<script>` block. Owned;
/// keys remain `String` to match the borrowed-form contract (cheap to
/// compare at the pre-lowered read sites).
#[derive(Debug, Clone)]
pub struct ResolvedElementsOwned {
    pub members: Vec<OwnedObjectMember>,
}

// ─────────────────────────────────────────────────────────────────────
// Type / span arenas
// ─────────────────────────────────────────────────────────────────────

/// Owned per-program decl arena. All payloads are `Send + Sync +
/// 'static` (no allocator-lifetime references). The arena fields are
/// flat `Vec`s for cache-line locality; lookups by id are O(1).
#[derive(Debug, Clone, Default)]
pub struct TypeDeclArena {
    pub aliases: Vec<OwnedAliasEntry>,
    pub interfaces: Vec<OwnedInterfaceEntry>,
    pub classes: Vec<OwnedClassDecl>,
    pub params: Vec<OwnedTypeParameter>,
    pub exprs: Vec<OwnedTypeExpr>,
}

impl TypeDeclArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_alias(&mut self, alias: OwnedAliasEntry) -> TypeAliasDeclId {
        let id = TypeAliasDeclId(self.aliases.len() as u32);
        self.aliases.push(alias);
        id
    }

    pub fn push_interface(&mut self, iface: OwnedInterfaceEntry) -> InterfaceDeclId {
        let id = InterfaceDeclId(self.interfaces.len() as u32);
        self.interfaces.push(iface);
        id
    }

    pub fn push_class(&mut self, class: OwnedClassDecl) -> ClassDeclId {
        let id = ClassDeclId(self.classes.len() as u32);
        self.classes.push(class);
        id
    }

    pub fn push_param(&mut self, param: OwnedTypeParameter) -> TypeParameterDeclId {
        let id = TypeParameterDeclId(self.params.len() as u32);
        self.params.push(param);
        id
    }

    pub fn push_expr(&mut self, expr: OwnedTypeExpr) -> TypeExprId {
        let id = TypeExprId(self.exprs.len() as u32);
        self.exprs.push(expr);
        id
    }

    pub fn alias(&self, id: TypeAliasDeclId) -> Option<&OwnedAliasEntry> {
        self.aliases.get(id.0 as usize)
    }

    pub fn interface(&self, id: InterfaceDeclId) -> Option<&OwnedInterfaceEntry> {
        self.interfaces.get(id.0 as usize)
    }

    pub fn class(&self, id: ClassDeclId) -> Option<&OwnedClassDecl> {
        self.classes.get(id.0 as usize)
    }

    pub fn param(&self, id: TypeParameterDeclId) -> Option<&OwnedTypeParameter> {
        self.params.get(id.0 as usize)
    }

    pub fn expr(&self, id: TypeExprId) -> Option<&OwnedTypeExpr> {
        self.exprs.get(id.0 as usize)
    }
}

/// Owned span arena. Spans are stored once and indexed via
/// [`SpanId`]-equivalent keys when the resolver wants to amortize span
/// storage across many references. Tier 1A keeps this minimal — the
/// real consumer is Tier 1B (BFS bridge).
#[derive(Debug, Clone, Default)]
pub struct SpanArena {
    pub spans: Vec<SpanId>,
}

impl SpanArena {
    pub fn new() -> Self {
        Self::default()
    }
}

// ─────────────────────────────────────────────────────────────────────
// Owned type-resolution context
// ─────────────────────────────────────────────────────────────────────

/// Owned, `Send + Sync + 'static` mirror of
/// `verter_parser::utils::oxc::vue::script::resolve_type::TypeResolutionContext`
/// minus borrowed AST pointers (D18 + D45 + D65). Stored in the
/// `TypeResolutionContextDb` (introduced empty in 1A; populated in
/// 1C-α).
///
/// **No `source: &'ctx [u8]` field** (D65 — the borrowed form had this
/// for byte-level identifier comparisons; the owned form uses
/// [`InternedIdentifierId`] equality instead, which is O(1) and allocation-
/// free).
#[derive(Debug, Clone)]
pub struct OwnedTypeResolutionContext {
    /// Local type alias declarations keyed by interned identifier id.
    /// Values pair the owned `TypeAliasDeclId` with the optional
    /// generic parameter declaration id.
    pub type_aliases:
        FxHashMap<InternedIdentifierId, (TypeAliasDeclId, Option<TypeParameterDeclId>)>,
    /// Local interface declarations keyed by interned identifier id.
    pub interfaces: FxHashMap<InternedIdentifierId, OwnedInterfaceEntry>,
    /// Local class declarations keyed by interned identifier id.
    pub classes: FxHashMap<InternedIdentifierId, OwnedClassDecl>,
    /// Generic type parameters: `(name_span, optional constraint expr)`.
    pub type_params: Vec<(SpanId, Option<TypeExprId>)>,
    /// Bound generic type parameters for the current instantiation:
    /// `(name_span, bound expr)`.
    pub type_param_bindings: Vec<(SpanId, TypeExprId)>,
    /// Stable cache-key representation of `type_param_bindings`.
    pub type_param_bindings_cache_key: Arc<[ResolvedTypeParamBindingCacheKey]>,
    /// Diagnostics collected during resolution.
    pub diagnostics: Vec<ResolutionDiagnostic>,
    /// Pre-resolved types from companion `<script>` block, keyed by
    /// type name string. Owned `String` keys preserve the
    /// borrowed-form contract.
    pub companion_types: FxHashMap<String, ResolvedElementsOwned>,
    /// Import origins for companion types. Keyed by type name, value is
    /// the package/module specifier the type was imported from.
    pub companion_origins: FxHashMap<String, String>,
    /// Owned decl arena for the lowered `TypeExpr` / interface / class /
    /// parameter / alias bodies referenced by ids.
    pub decl_arena: TypeDeclArena,
    /// Owned span arena.
    pub span_arena: SpanArena,
    /// Declaration fingerprint table built at lowering time (D104). Tier
    /// 1B's TypeHandle resolution looks up
    /// [`DeclarationFingerprint`] -> [`DeclId`] in this map.
    pub declaration_fingerprints: FxHashMap<DeclarationFingerprint, DeclId>,
}

impl OwnedTypeResolutionContext {
    /// Construct an empty context. Used by Tier 1A's empty-DB shape and
    /// by tests; Tier 1C-α populates the real context from lowering.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            type_aliases: FxHashMap::default(),
            interfaces: FxHashMap::default(),
            classes: FxHashMap::default(),
            type_params: Vec::new(),
            type_param_bindings: Vec::new(),
            type_param_bindings_cache_key: Arc::from(Vec::<ResolvedTypeParamBindingCacheKey>::new()),
            diagnostics: Vec::new(),
            companion_types: FxHashMap::default(),
            companion_origins: FxHashMap::default(),
            decl_arena: TypeDeclArena::new(),
            span_arena: SpanArena::new(),
            declaration_fingerprints: FxHashMap::default(),
        }
    }
}

// Compile-time `Send + Sync + 'static` guard. Discriminating test
// `owned_type_resolution_context_is_send_sync_static` (in
// `type_resolution_context_tests.rs`) asserts the same property at
// runtime via `assert_impl_all!`.
const _: fn() = || {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<OwnedTypeResolutionContext>();
    assert_send_sync_static::<TypeDeclArena>();
    assert_send_sync_static::<OwnedInterfaceEntry>();
    assert_send_sync_static::<OwnedClassDecl>();
    assert_send_sync_static::<OwnedTypeExpr>();
};
