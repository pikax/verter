//! Exact, content-free override-identity cache key for fallthrough.
//!
//! The fallthrough cache-key `overrides` dimension must reuse a warm
//! fallthrough surface ONLY for an EQUIVALENT child prop-type override set.
//! The override set is carried in node domain
//! ([`crate::resolver_core::FallthroughPropOverrideSet`]); its identity is the
//! sealed [`FallthroughOverrideIdentity`] computed once at set construction.
//!
//! Identity is the FULL structural projection of each override value node onto a
//! content-free key — NOT a lossy `u64` digest. A `u64` digest dropped
//! [`crate::semantic_query::SemanticNodeData::IndexedAccess`]'s `index`,
//! signatures and carrier type-args and truncated at a fixed depth, so two
//! genuinely-different override sets could collide and reuse the WRONG warm
//! surface. The projection here:
//!
//! - is EXHAUSTIVE over the value-node shape (the projector that fills these
//!   keys has NO wildcard arm — a new node variant fails compilation until
//!   classified);
//! - is R6-compliant: it carries NO `whole_hash` / `content_hash` /
//!   `parse_stable_hash` / raw [`crate::semantic_query::SemanticNodeId`]. A
//!   declaration reference projects through the env-bearing, content-free
//!   [`crate::semantic_query::ResolvedDeclSlotIdentity`] /
//!   [`crate::semantic_query::ValueRootSlotIdentity`] slots; a `BareRef` scope
//!   drops the scope's `whole_hash`;
//! - compares STRUCTURE, never a digest alone — every key type derives
//!   `PartialEq`/`Eq`/`Hash` over its stored fields, so equality is the
//!   structural comparison and no digest aliasing is possible.

use std::sync::Arc;

use crate::semantic_query::index_key::CanonicalIndexInt;
use crate::semantic_query::{
    MapperKind, MemberMergeRole, NodeScopeId, OptionalityMod, PrimitiveKind, QueryError,
    ReadonlyMod, ResolvedDeclSlotIdentity, SemanticQueryValueTag, SyntheticBindingId,
    ValueRootSlotIdentity,
};
use verter_type_expr::{LiteralValue, MemberVisibility};

/// Sealed (closed) override-identity cache-key dimension (codex ruling C):
/// exact canonical identity for a representable set, plus a typed
/// uncacheable fallback.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum FallthroughOverrideIdentity {
    /// The empty override set canonicalizes here. A `None` override set and
    /// an empty set are the same identity.
    #[default]
    NoOverrides,
    /// The exact canonical identity of a non-empty, fully-representable
    /// override set.
    Exact(Arc<FallthroughOverrideSetKey>),
    /// CONSTRUCTION RESULT ONLY — never a meaningful stored key value. Produced
    /// when an override value projects to an unrepresentable node
    /// ([`crate::semantic_query::SemanticNodeData::VueMacroElements`], a cycle
    /// anomaly, or a traversal/budget cap). A request whose override identity is
    /// `Uncacheable` skips override-bearing fallthrough cache admission AND
    /// singleflight for that request (it recomputes cold every time) so two
    /// genuinely-different unrepresentable override sets can never alias.
    Uncacheable,
}

impl FallthroughOverrideIdentity {
    /// The override identity for an optional override set: `None` →
    /// [`Self::NoOverrides`]; `Some(set)` → the set's pre-computed identity
    /// (canonicalized at construction time, so an empty `Some(set)` is also
    /// [`Self::NoOverrides`]).
    #[must_use]
    pub fn for_overrides(
        overrides: Option<&crate::resolver_core::FallthroughPropOverrideSet>,
    ) -> Self {
        match overrides {
            None => Self::NoOverrides,
            Some(set) => set.identity.clone(),
        }
    }
}

/// The exact identity of a non-empty override set: its `(prop_name, value
/// key)` entries SORTED by prop name and made UNIQUE by keeping the
/// runtime-effective (first-match) winner per name — mirroring
/// [`crate::resolver_core::FallthroughPropOverrideSet::lookup`], which is
/// first-match / order-sensitive. Two sets with the same effective overrides
/// in any source order produce the same key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideSetKey {
    pub entries: Vec<(Arc<str>, FallthroughOverrideValueKey)>,
}

/// Content-free structural projection of an override value node's
/// [`crate::semantic_query::SemanticNodeData`] shape. Every recursive position
/// holds a fully-projected child key (not a node id), so equality compares the
/// whole structure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FallthroughOverrideValueKey {
    Alias(Box<FallthroughOverrideValueKey>),
    Object(Box<FallthroughOverrideSurfaceKey>),
    Union(Vec<FallthroughOverrideValueKey>),
    Intersection(Vec<FallthroughOverrideValueKey>),
    Primitive(PrimitiveKind),
    Literal(LiteralValue),
    Opaque(OpaqueErrorKey),
    Array {
        element: Box<FallthroughOverrideValueKey>,
        readonly: bool,
    },
    Tuple {
        elements: Vec<FallthroughOverrideTupleElementKey>,
        readonly: bool,
    },
    TemplateLiteral {
        quasis: Vec<Arc<str>>,
        expressions: Vec<FallthroughOverrideValueKey>,
    },
    KeyOf {
        base: Box<FallthroughOverrideValueKey>,
    },
    IndexedAccess {
        object: Box<FallthroughOverrideValueKey>,
        index: FallthroughOverrideIndexKey,
    },
    Mapped(Box<FallthroughOverrideMappedKey>),
    TypeOf {
        value_root: ValueRootSlotIdentity,
        path: Vec<Arc<str>>,
        type_args: Vec<FallthroughOverrideValueKey>,
    },
    TypeParam {
        decl: ResolvedDeclSlotIdentity,
        param_index: u16,
        constraint: Option<Box<FallthroughOverrideValueKey>>,
        default: Option<Box<FallthroughOverrideValueKey>>,
    },
    Infer {
        name: Arc<str>,
    },
    MergedDecl {
        contributors: Vec<FallthroughOverrideValueKey>,
    },
    Conditional(Box<FallthroughOverrideConditionalKey>),
    Function(Box<FallthroughOverrideFunctionKey>),
    DeclRef {
        slot: ResolvedDeclSlotIdentity,
    },
    InstantiationRef {
        base: ResolvedDeclSlotIdentity,
        args: Vec<FallthroughOverrideValueKey>,
    },
    BareRef {
        name: Arc<str>,
        scope: FallthroughOverrideScopeKey,
        type_args: Vec<FallthroughOverrideValueKey>,
    },
    ImportType {
        specifier: Arc<str>,
        qualifier: Vec<Arc<str>>,
        typeof_query: bool,
        type_args: Vec<FallthroughOverrideValueKey>,
    },
    RawFallback {
        raw: Arc<str>,
    },
    ConstructorType {
        signature: Box<FallthroughOverrideValueKey>,
    },
    SyntheticBinding {
        id: SyntheticBindingId,
    },
}

/// Object-surface projection. Member ordering / signatures / index signatures
/// are all carried; span / declaration-origin / macro provenance are dropped
/// (origin-only, not part of the type's structural identity).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideSurfaceKey {
    pub members: Vec<FallthroughOverrideMemberKey>,
    pub index_signatures: Vec<FallthroughOverrideIndexSigKey>,
    pub call_signatures: Vec<FallthroughOverrideValueKey>,
    pub construct_signatures: Vec<FallthroughOverrideValueKey>,
    pub keyspace: Option<Box<FallthroughOverrideValueKey>>,
    pub has_index_signature: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideMemberKey {
    pub name: Arc<str>,
    pub value: FallthroughOverrideValueKey,
    pub optional: bool,
    pub readonly: bool,
    pub is_method: bool,
    pub visibility: MemberVisibility,
    pub merge_role: MemberMergeRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideIndexSigKey {
    pub key_type: FallthroughOverrideValueKey,
    pub value_type: FallthroughOverrideValueKey,
    pub readonly: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideTupleElementKey {
    pub label: Option<Arc<str>>,
    pub value: FallthroughOverrideValueKey,
    pub optional: bool,
    pub rest: bool,
}

/// The indexed-access INDEX operand — the field a `u64` digest dropped, which
/// caused the cache-poison: `T["a"]` and `T["b"]` now key DISTINCTLY.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FallthroughOverrideIndexKey {
    String(Arc<str>),
    Number(CanonicalIndexInt),
    TypeNode(Box<FallthroughOverrideValueKey>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideMappedKey {
    pub source: FallthroughOverrideValueKey,
    pub parameter_node: FallthroughOverrideValueKey,
    pub key_space: FallthroughOverrideValueKey,
    pub value_expr: FallthroughOverrideValueKey,
    pub optionality: OptionalityMod,
    pub readonly: ReadonlyMod,
    pub name_remap: Option<FallthroughOverrideValueKey>,
    pub kind: MapperKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideConditionalKey {
    pub check: FallthroughOverrideValueKey,
    pub extends: FallthroughOverrideValueKey,
    pub true_branch: FallthroughOverrideValueKey,
    pub false_branch: FallthroughOverrideValueKey,
    pub distributive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideFunctionKey {
    pub params: Vec<FallthroughOverrideParamKey>,
    pub return_type: FallthroughOverrideValueKey,
    pub type_parameters: Vec<FallthroughOverrideTypeParamDeclKey>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideParamKey {
    pub name: Option<Arc<str>>,
    pub ty: FallthroughOverrideValueKey,
    pub optional: bool,
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FallthroughOverrideTypeParamDeclKey {
    pub name: Arc<str>,
    pub constraint: Option<FallthroughOverrideValueKey>,
    pub default: Option<FallthroughOverrideValueKey>,
}

/// Content-free projection of a `BareRef` carrier scope. The scope's
/// `whole_hash` is dropped (R6 — the key carries no content/version hash);
/// the canonical file id + optional local-scope index are content-free.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FallthroughOverrideScopeKey {
    Global,
    File {
        canonical_id: Arc<str>,
        local_scope: Option<u32>,
    },
}

impl FallthroughOverrideScopeKey {
    #[must_use]
    pub fn from_node_scope(scope: &NodeScopeId) -> Self {
        match scope {
            NodeScopeId::Global => Self::Global,
            NodeScopeId::File {
                canonical_id,
                local_scope,
                ..
            } => Self::File {
                canonical_id: Arc::clone(canonical_id),
                local_scope: *local_scope,
            },
        }
    }
}

/// Content-free projection of an [`crate::semantic_query::QueryError`] carried
/// on an `Opaque` override value. Runtime / version-bearing detail
/// (`BudgetExceededFailure`, `DeclPlaceholder.whole_hash`) is dropped; the
/// variant discriminant plus content-free name fields are retained so two
/// distinct opaque overrides stay distinct.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OpaqueErrorKey {
    Miss,
    UnsupportedIntrinsic {
        name: Arc<str>,
    },
    BudgetExceeded,
    UnstableState {
        attempts: u8,
    },
    AliasCycle {
        chain: Vec<Arc<str>>,
    },
    RecursiveRef {
        name: Arc<str>,
    },
    Other(Arc<str>),
    DeclPlaceholder {
        canonical_id: Arc<str>,
        name: Arc<str>,
    },
    ValueDomainMismatch {
        expected: SemanticQueryValueTag,
        actual: SemanticQueryValueTag,
    },
    RaiseAliasCycle,
    TypeParamCycle,
    RaiseMiss,
    UnrepresentableSurface,
    UnrepresentableSurfaceMember,
    VueMacroElementsPlaceholder,
}

impl OpaqueErrorKey {
    /// Project a `QueryError` to its content-free key. Exhaustive (no
    /// wildcard) so a new `QueryError` variant is classified here.
    #[must_use]
    pub fn from_query_error(err: &QueryError) -> Self {
        match err {
            QueryError::Miss => Self::Miss,
            QueryError::UnsupportedIntrinsic { name } => Self::UnsupportedIntrinsic {
                name: Arc::clone(name),
            },
            QueryError::BudgetExceeded(_) => Self::BudgetExceeded,
            QueryError::UnstableState { attempts } => Self::UnstableState {
                attempts: *attempts,
            },
            QueryError::AliasCycle { chain } => Self::AliasCycle {
                chain: chain.iter().cloned().collect(),
            },
            QueryError::RecursiveRef { name } => Self::RecursiveRef {
                name: Arc::clone(name),
            },
            QueryError::Other(text) => Self::Other(Arc::clone(text)),
            // `whole_hash` is DROPPED (R6): the key is content-free.
            QueryError::DeclPlaceholder {
                canonical_id, name, ..
            } => Self::DeclPlaceholder {
                canonical_id: Arc::clone(canonical_id),
                name: Arc::clone(name),
            },
            QueryError::ValueDomainMismatch { expected, actual } => Self::ValueDomainMismatch {
                expected: *expected,
                actual: *actual,
            },
            QueryError::RaiseAliasCycle => Self::RaiseAliasCycle,
            QueryError::TypeParamCycle => Self::TypeParamCycle,
            QueryError::RaiseMiss => Self::RaiseMiss,
            QueryError::UnrepresentableSurface => Self::UnrepresentableSurface,
            QueryError::UnrepresentableSurfaceMember => Self::UnrepresentableSurfaceMember,
            QueryError::VueMacroElementsPlaceholder => Self::VueMacroElementsPlaceholder,
        }
    }
}
