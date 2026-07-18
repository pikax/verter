//! The owner-layer shape engine: ONE private generic `SemanticNodeData`
//! traversal parameterized by an output algebra.
//!
//! This is the single exhaustive `SemanticNodeData` fold that the raise side
//! owns. It replaces the former bare `raise_node_to_type_expr_core_impl` match:
//! the per-arm CONTROL FLOW (the `?` aborts, the `filter_map` drops, the
//! Intersection arm-drop + 0/1/many collapse, the Object empty-vs-surface
//! split with a fresh per-member cycle set, the carrier `<raise miss>`
//! defaults, the `Alias`/`TypeParam` cycle guards) lives ONCE in
//! [`fold_node`]; only the per-arm LEAF CONSTRUCTION differs per algebra. So
//! the materialization and the node-domain facts/key can never drift — there
//! is exactly ONE traversal, anti-drift is structural.
//!
//! Three algebras:
//! - [`MaterializeTypeExprAlg`] (`Out = TypeExpr`) — the EXACT historical
//!   materialization, reached ONLY through the sealed `OutputProjector` output
//!   seam ([`super::ProjectSemanticDispatch::raise_node_to_type_expr`]) and the
//!   `#[cfg(test)]` oracle.
//! - [`RaisedShapeAlg`] (`Out = RaisedShapeResult`) — the TRUE bottom-up
//!   facts/key, computed WITHOUT allocating a `TypeExpr`.
//! - [`TypeExprShapeAlg`] (folds an existing `&TypeExpr`) — produces the SAME
//!   [`RaisedShapeKey`] key space so a node's raised shape can be compared
//!   against a caller's input `TypeExpr` without materializing the node.
//!
//! The opaque [`RaisedShapeKey`] is an INTERNED STRUCTURAL term (a hash-consed
//! raised-shape DAG): a prehash fast-path for cheap comparison PLUS structural
//! equality for collision correctness — it replaces `TypeExpr`'s derived
//! `PartialEq` in the route fixpoint, so a hash-only digest is not exact
//! enough. The interner is per evaluation; equality of two keys is an interned
//! id comparison.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_span::Span;
use verter_type_expr::{LiteralValue, MappedModifier, MemberVisibility, PrimitiveName, TypeExpr};

use super::super::ProjectSemanticDispatch;
use crate::semantic_query::{IndexKey, QueryError, SemanticNodeData, SemanticNodeId, SurfaceView};

// Algebra impls + leaf conversions split into child files for file-size (the
// fold + the algebra trait + the interned term stay here, in the parent module).
mod conversions;
mod materialize;
mod node_domain;
mod publication;

pub(crate) use conversions::semantic_primitive_to_primitive_name;
use conversions::{mapped_modifier_for_optionality, mapped_modifier_for_readonly};
pub(in crate::project_semantic_dispatch) use materialize::fold_to_type_expr;
pub(in crate::project_semantic_dispatch) use node_domain::node_is_unknown_materializing_failure;
use node_domain::{type_expr_to_key, RaisedFactsAlg, RaisedShapeAlg};
pub(in crate::project_semantic_dispatch) use publication::project_node_publication_score;
#[cfg(test)]
pub(in crate::project_semantic_dispatch) use publication::type_expr_publication_score;

// ===========================================================================
// The interned structural raised-shape term + key.
// ===========================================================================

/// The interned structural identity of a raised shape — the comparison subject
/// that replaces `TypeExpr`'s derived `PartialEq` for mid-flight decisions.
///
/// An id into a per-evaluation [`ShapeInterner`]. Interning dedups structurally
/// (the interner keys on the full [`RaisedTerm`]), so id equality is exactly
/// structural equality of the raised shape, with the id acting as the prehash
/// fast-path. Two keys are comparable ONLY when minted by the SAME interner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::project_semantic_dispatch) struct RaisedShapeKey(u32);

/// Closed shallow member-value vocabulary projected directly from a node's
/// normalized raised shape, without allocating a `TypeExpr`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RaisedShallowMemberOutput {
    Primitive(PrimitiveName),
    Literal(LiteralValue),
    Ref { name: Arc<str> },
    EmptyObject,
    Opaque,
}

/// A faithful structural mirror of the raised `TypeExpr` shape, with nested
/// types replaced by interned [`RaisedShapeKey`] child handles so the term is
/// built bottom-up WITHOUT allocating a `TypeExpr`. Carries EXACTLY the fields
/// `TypeExpr`'s derived `PartialEq` distinguishes (names, literals, spans,
/// visibility, modifiers, raw strings, synthetic-carrier identity), so two
/// shapes intern to the same key iff their materialized `TypeExpr`s would be
/// `==`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RaisedTerm {
    Primitive(PrimitiveName),
    Literal(LiteralValue),
    Union(Vec<RaisedShapeKey>),
    Intersection(Vec<RaisedShapeKey>),
    Array {
        element: RaisedShapeKey,
        readonly: bool,
    },
    Tuple {
        elements: Vec<RaisedTupleElement>,
        readonly: bool,
    },
    Object(Vec<RaisedObjectMember>),
    Function(RaisedFunction),
    ConstructorType(RaisedFunction),
    Ref {
        name: Arc<str>,
        type_arguments: Vec<RaisedShapeKey>,
    },
    TypeParameter {
        name: Arc<str>,
        constraint: Option<RaisedShapeKey>,
        default: Option<RaisedShapeKey>,
    },
    KeyOf(RaisedShapeKey),
    TypeOf {
        path: Vec<Arc<str>>,
        type_args: Vec<RaisedShapeKey>,
    },
    IndexedAccess {
        object: RaisedShapeKey,
        index: RaisedShapeKey,
    },
    Conditional {
        check: RaisedShapeKey,
        extends: RaisedShapeKey,
        true_type: RaisedShapeKey,
        false_type: RaisedShapeKey,
    },
    Mapped {
        parameter: Arc<str>,
        source: RaisedShapeKey,
        value: RaisedShapeKey,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<RaisedShapeKey>,
    },
    TemplateLiteral {
        quasis: Vec<Arc<str>>,
        expressions: Vec<RaisedShapeKey>,
    },
    Infer {
        name: Arc<str>,
    },
    /// Standalone `readonly` / rest `...T` (only reachable through
    /// [`type_expr_to_key`] over a caller-supplied `&TypeExpr`; the raiser
    /// never produces it).
    Rest(RaisedShapeKey),
    /// Parenthesized fidelity wrapper (only reachable through
    /// [`type_expr_to_key`]; the raiser never produces it). Kept DISTINCT from
    /// its inner so `Parenthesized(X)` never compares equal to `X`, matching
    /// `TypeExpr`'s derived `PartialEq`.
    Parenthesized(RaisedShapeKey),
    RecursiveRef {
        name: Arc<str>,
        type_arguments: Vec<RaisedShapeKey>,
        conditional_context: Vec<RaisedRecursiveFrame>,
    },
    SyntheticSlotBinding(Arc<verter_type_expr::SyntheticCarrierKey>),
    ImportType {
        specifier: Arc<str>,
        qualifier: Vec<Arc<str>>,
        typeof_query: bool,
        type_arguments: Vec<RaisedShapeKey>,
    },
    Unknown {
        raw: Arc<str>,
    },
}

/// Tuple element mirror — label/optional/rest carried verbatim, the element
/// type held as a child key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RaisedTupleElement {
    label: Option<Arc<str>>,
    ty: RaisedShapeKey,
    optional: bool,
    rest: bool,
}

/// `RecursiveConditionalFrame` mirror — branch/decided carried verbatim,
/// check/extends held as child keys. Only reachable through
/// [`type_expr_to_key`] (the raiser produces `recursive_ref(name, [])` with no
/// frames).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RaisedRecursiveFrame {
    branch: verter_type_expr::RecursiveConditionalBranch,
    decided: bool,
    check: RaisedShapeKey,
    extends: RaisedShapeKey,
}

/// Object member mirror — one variant per `ObjectMember`, with the member type
/// (or function signature) held as child keys and all identity-bearing leaf
/// metadata (name, visibility, optional/readonly, spans) carried verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RaisedObjectMember {
    Property {
        name: Arc<str>,
        ty: RaisedShapeKey,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    },
    Method {
        name: Arc<str>,
        function: RaisedFunction,
        optional: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    },
    CallSignature(RaisedFunction),
    ConstructSignature(RaisedFunction),
    IndexSignature {
        key_name: Arc<str>,
        key_type: RaisedShapeKey,
        value_type: RaisedShapeKey,
        readonly: bool,
        spans: verter_type_expr::IndexSignatureSpans,
    },
}

/// Function-shape mirror — parameters/return/type-params held as child keys
/// with leaf metadata (names, optional/rest, spans) verbatim so the key
/// distinguishes exactly what `FunctionExpr`'s hand-written `PartialEq` does
/// (which excludes each param's `has_ts_annotation` — see [`RaisedFunctionParam`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RaisedFunction {
    parameters: Vec<RaisedFunctionParam>,
    return_type: Option<RaisedShapeKey>,
    type_parameters: Vec<RaisedTypeParam>,
    signature_span: Option<Span>,
    return_type_span: Option<Span>,
}

/// Function-parameter mirror. It carries EXACTLY the fields
/// [`verter_type_expr::FunctionParam`]'s hand-written `PartialEq`/`Eq`/`Hash`
/// distinguish — name, ty, optional, rest, span — and DELIBERATELY OMITS
/// `has_ts_annotation`. That field is a transient lowering-time gate (JSDoc
/// `@param` precedence), NOT semantic type identity; `FunctionParam`'s
/// hand-written eq excludes it, so the raised key MUST omit it to stay EXACTLY
/// `TypeExpr::PartialEq`-equivalent. The exclusion is structural-by-absence
/// here: there is no field to accidentally fold into the derived key, so a
/// node-raised param (annotation `false`) and a `TypeExpr` input param
/// (annotation `true`) that `TypeExpr::PartialEq` treats as equal intern to the
/// SAME key (otherwise a no-op reads as "changed" at the route gates).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RaisedFunctionParam {
    name: Option<Arc<str>>,
    ty: RaisedShapeKey,
    optional: bool,
    rest: bool,
    span: Option<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RaisedTypeParam {
    name: Arc<str>,
    constraint: Option<RaisedShapeKey>,
    default: Option<RaisedShapeKey>,
}

/// Per-evaluation hash-cons interner: maps a [`RaisedTerm`] to a stable
/// [`RaisedShapeKey`] id, deduping structurally so equal shapes share an id.
#[derive(Default)]
pub(in crate::project_semantic_dispatch) struct ShapeInterner {
    table: FxHashMap<Arc<RaisedTerm>, RaisedShapeKey>,
    terms: Vec<Arc<RaisedTerm>>,
}

impl ShapeInterner {
    fn intern(&mut self, term: RaisedTerm) -> RaisedShapeKey {
        if let Some(key) = self.table.get(&term) {
            return *key;
        }
        let term = Arc::new(term);
        let key = RaisedShapeKey(
            u32::try_from(self.terms.len()).expect("raised-shape interner id space"),
        );
        self.terms.push(Arc::clone(&term));
        self.table.insert(term, key);
        key
    }

    fn term(&self, key: RaisedShapeKey) -> &RaisedTerm {
        &self.terms[key.0 as usize]
    }
}

// ===========================================================================
// Bottom-up facts computed alongside the key — NO `TypeExpr` allocation.
// ===========================================================================

/// The publication-scoring facts of a raised shape — the inputs the
/// publication-finaliser comparison ([`crate::meta_resolve::compare_node_improvement`]
/// / [`crate::meta_resolve::compare_type_expr_improvement`]) reads. Computed
/// bottom-up by the [`publication`] algebra; `symbolic_carriers` / `generic_detail`
/// are WHOLE-TREE sums while `structural_top_level` / `exact_unknown_root` are
/// ROOT-only (set by the outermost arm, never propagated from a child). Defined
/// HERE (not in the `publication` child) so the one-hop `raise` crate re-export
/// resolves it — `pub(crate)` because the comparison formula lives in
/// `crate::meta_resolve::scoring`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PublicationScore {
    /// The symbolic-carrier penalty — `count_symbolic_carriers_in_expr(raise(node))`:
    /// each reference carrier (`Ref` / `ImportType` / `RecursiveRef`),
    /// `IndexedAccess` / `Conditional` / `Mapped` / `TemplateLiteral`, and each
    /// `TypeOf` / `TypeParameter` / `SyntheticSlotBinding` / `Infer` / `Unknown`
    /// leaf costs `+1`; compound shapes recurse without a self-penalty.
    pub(crate) symbolic_carriers: usize,
    /// The type-parameter detail — `count_generic_detail_in_expr(raise(node))`: a
    /// declared `TypeParameter` (standalone OR on a function signature) costs
    /// `+1` plus its constraint/default detail.
    pub(crate) generic_detail: usize,
    /// Whether the raised ROOT is a concrete structural shape (NOT a symbolic
    /// reference / operator carrier) — `type_expr_has_structural_top_level(raise(node))`.
    pub(crate) structural_top_level: bool,
    /// Whether the raised ROOT term is exactly `TypeExpr::Unknown { .. }` —
    /// `matches!(raise(node), TypeExpr::Unknown { .. })`, the first clause of the
    /// improvement comparison. A `RecursiveRef` root is NOT unknown (it raises to
    /// `TypeExpr::RecursiveRef`, a structural carrier).
    pub(crate) exact_unknown_root: bool,
}

/// Facts about the shape a node raises to, derived bottom-up from the
/// POST-NORMALIZED raised shape (NOT the raw graph kind). The TYPE is
/// `pub(crate)` because it is re-exported from `raise` and read by the Kind-B
/// callers; the FIELDS are PRIVATE (defining-module + descendants only).
///
/// SEALED: a value with a passing fact can be produced ONLY by the node-domain
/// fold's [`summary`](node_domain) constructor layer (the sole construction site
/// lives in the `node_domain` child). No sibling can fabricate a
/// `RaisedShapeFacts { materialized: true, expanded_surface: true, .. }` struct
/// literal — the FIELDS are PRIVATE (`error[E0451]`). These facts are NOT
/// themselves a [`route_admission`](crate::resolver_core::component_meta_query_engine)
/// admission input: a mint helper takes the node-bound [`RaisedNodeShapeFacts`]
/// witness (or [`NodeShapeEq`]), so a passing fact can only reach a gate PAIRED
/// with the node it was computed for — a free `(facts, node)` mispair is
/// unrepresentable. Cross-module readers go through the `pub(crate)` getters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RaisedShapeFacts {
    /// `true` for every node the fold produces a value for — i.e.
    /// `raise(node).is_some()`. A `Some(result)` always has
    /// `can_shell_raise == true`.
    can_shell_raise: bool,
    /// `dispatch_route_expr_is_materialized(raise(node))`: the structural AND
    /// over all value-bearing children, `Unknown { raw }` materialized iff
    /// `!raw_is_unmaterialized_sentinel(raw)`.
    materialized: bool,
    /// `type_expr_is_expanded_surface(raise(node))`: `false` only when the
    /// raised root (recursing through `Union`/`Intersection`) is an open
    /// deferred shell (`KeyOf`/`IndexedAccess`/`Mapped`/`TypeOf`/`Conditional`).
    expanded_surface: bool,
}

impl RaisedShapeFacts {
    /// `true` for every node the fold produces a value for (`raise(node).is_some()`).
    #[must_use]
    pub(crate) fn can_shell_raise(&self) -> bool {
        self.can_shell_raise
    }
    /// `dispatch_route_expr_is_materialized(raise(node))` — the structural AND
    /// over all value-bearing children.
    #[must_use]
    pub(crate) fn materialized(&self) -> bool {
        self.materialized
    }
    /// `type_expr_is_expanded_surface(raise(node))` — `false` only for an open
    /// deferred shell root.
    #[must_use]
    pub(crate) fn expanded_surface(&self) -> bool {
        self.expanded_surface
    }
}

/// A NODE-BOUND raised-shape facts witness: the [`RaisedShapeFacts`] of a node
/// PAIRED with the `SemanticNodeId` they were computed for, in ONE sealed value.
/// This is the SOLE cross-module input to the
/// [`route_admission`](crate::resolver_core::component_meta_query_engine) carrier
/// mint helpers — each helper takes ONLY this witness and mints the carrier from
/// `witness.node()`, never a free `(facts, node)` pair.
///
/// SEALED + node-bound (the carrier-proof terminal): the FIELDS are PRIVATE and a
/// value is constructed ONLY inside [`shape_engine`](self) (the node-domain fold's
/// projection entry points bind the queried node to its own computed facts). There
/// is no constructor accepting a free `SemanticNodeId`, and a sibling struct
/// literal is `error[E0451]: field … is private` — so a caller cannot pair node
/// A's facts with node B. The `admit_*` helpers mint the carrier from
/// `witness.node()` ALONE, so the node a carrier holds is ALWAYS the node whose
/// OWN gate the witness facts passed. Cross-module readers use the `pub(crate)`
/// passthrough getters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RaisedNodeShapeFacts {
    node: SemanticNodeId,
    facts: RaisedShapeFacts,
}

impl RaisedNodeShapeFacts {
    /// The node these facts were computed for — the ONLY admission input a
    /// `route_admission` mint helper reads to bind the carrier it mints.
    #[must_use]
    pub(crate) fn node(&self) -> SemanticNodeId {
        self.node
    }
    /// `dispatch_route_expr_is_materialized(raise(node))` — passthrough to the
    /// inner [`RaisedShapeFacts`].
    #[must_use]
    pub(crate) fn materialized(&self) -> bool {
        self.facts.materialized()
    }
    /// `type_expr_is_expanded_surface(raise(node))` — passthrough to the inner
    /// [`RaisedShapeFacts`].
    #[must_use]
    pub(crate) fn expanded_surface(&self) -> bool {
        self.facts.expanded_surface()
    }
    /// `raise(node).is_some()` — passthrough to the inner [`RaisedShapeFacts`].
    #[must_use]
    pub(crate) fn can_shell_raise(&self) -> bool {
        self.facts.can_shell_raise()
    }
}

/// The exact raised-term CLASS a folded node-domain value belongs to, as far as
/// the fold's three structural inspections care: the Intersection arm-drop
/// (`is_object_surface_sentinel` / `is_empty_object`) and the `ConstructorType`
/// rewrap + Object signature-member extraction (`out_as_function`). The
/// key-bearing [`RaisedShapeAlg`] could re-derive these by reading its interner
/// table, but the facts-only [`node_domain::RaisedFactsAlg`] has NO interner —
/// so the tag travels in the fold value and BOTH algebras answer the three
/// inspections from it identically. The full algebra's interner-readback only
/// ever checks these same three classes, so the tag is exactly as
/// discriminating (see the per-arm tag-placement rules in [`node_domain`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::project_semantic_dispatch) enum FactShapeTag {
    /// The folded value is a `Function` shape (NOT `ConstructorType`: the
    /// constructor rewrap reads the SIGNATURE child, never the constructor
    /// itself, so a constructor tags `Other`).
    Function,
    /// The `Unknown { raw == SEMANTIC_OBJECT_SURFACE }` sentinel arm (dropped
    /// from an intersection).
    ObjectSurfaceSentinel,
    /// The representable empty object `{}` (`{} & X ≡ X`).
    EmptyObject,
    /// Any other shape — invisible to the three inspections.
    Other,
}

/// The NORMALIZED raised-ROOT term class of a folded node — the node-domain
/// successor of the raw-node `match data.as_ref()` root mirrors
/// (`node_root_is_published_operator` / `node_root_is_typeof` /
/// `node_raises_to_object_surface` / `node_is_indexed_access_shell`).
///
/// Carried on [`RaisedShapeSummary`] and produced BOTTOM-UP by the same
/// [`summary`](node_domain) constructor layer that already folds the facts/tag.
/// Because the fold has ALREADY applied the structural normalizations the
/// materializer applies (the Intersection sentinel / empty-object arm drop +
/// single-arm collapse), reading the ROOT node's `root_kind` answers IDENTICALLY
/// to the matching `TypeExpr` predicate applied to `raise(node)` — i.e.
/// `node_pred(node) == type_expr_pred(raise(node))` BY CONSTRUCTION, with no
/// second arm-collapse walk and no `TypeExpr` materialised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::project_semantic_dispatch) enum RaisedRootKind {
    /// Raises to `TypeExpr::Object` — an `Object` / representable empty object,
    /// and the `MergedDecl` carrier that folds through the object constructors.
    Object,
    /// Raises to `TypeExpr::Ref` — a `DeclRef` / `InstantiationRef` / `BareRef` /
    /// `DeclPlaceholder` carrier.
    Reference,
    /// Raises to `TypeExpr::KeyOf`.
    KeyOf,
    /// Raises to `TypeExpr::IndexedAccess`.
    IndexedAccess,
    /// Raises to `TypeExpr::Conditional`.
    Conditional,
    /// Raises to `TypeExpr::TypeOf`.
    TypeOf,
    /// Raises to `TypeExpr::Mapped`; carries whether the mapped VALUE's OWN raised
    /// root is EXACTLY the `semanticMiss` sentinel — the single carrier the
    /// published-operator predicate suppresses (it PUBLISHES for any other value).
    Mapped { value_is_semantic_miss: bool },
    /// Any other raised root — `Union` / `Intersection` / primitive / literal /
    /// `Function` / `ConstructorType` / `Array` / `Tuple` / sentinel / `ImportType`
    /// / … — none of which is an object root, a reference, an operator, or a typeof
    /// for the root mirrors.
    Other,
}

/// The shared bottom-up SUMMARY of a raised shape: the [`RaisedShapeFacts`] plus
/// the [`FactShapeTag`]. The fact + tag FORMULAS live ONCE in the
/// [`node_domain`] summary-constructor layer; BOTH the key-bearing
/// [`RaisedShapeAlg`] and the facts-only `RaisedFactsAlg` build their per-arm
/// values through that one layer, so the two can never drift (parity is
/// structural, not test-enforced).
#[derive(Debug, Clone, Copy)]
pub(in crate::project_semantic_dispatch) struct RaisedShapeSummary {
    pub(in crate::project_semantic_dispatch) facts: RaisedShapeFacts,
    pub(in crate::project_semantic_dispatch) tag: FactShapeTag,
    /// `true` when this node's OWN raised term is an unmaterialised sentinel
    /// (`Unknown { raw }` / `Opaque(QueryError)` whose raw reads unmaterialised)
    /// — the node-domain equivalent of `type_expr_root_is_unmaterialized_sentinel`
    /// applied to the ROOT term only. Distinct from `facts.materialized`, which
    /// is the AND over all value-bearing children: a materialised object with a
    /// nested miss member has `materialized == false` but
    /// `root_unmaterialized_sentinel == false` (the root is the object, not a
    /// sentinel). Carried on the summary (not `RaisedShapeFacts`) because only the
    /// ROOT node's value is read, through [`project_node_root_sentinel`].
    pub(in crate::project_semantic_dispatch) root_unmaterialized_sentinel: bool,
    /// `true` when this node's OWN raised root term is EXACTLY the
    /// [`SEMANTIC_MISS`](crate::resolver_core::component_meta_query_engine::SEMANTIC_MISS)
    /// sentinel — strictly NARROWER than [`Self::root_unmaterialized_sentinel`]
    /// (which is also `true` for the object-surface / surface-member / budget /
    /// cycle / … spellings). Set ONLY by the two sentinel-leaf constructors
    /// (`unknown` / `opaque_sentinel`) when the raw / `QueryError` reads as the miss
    /// spelling, so it is the node-domain equivalent of `matches!(raise(node),
    /// TypeExpr::Unknown { raw } if raw == "semanticMiss")` applied to the ROOT term.
    /// Read by the `mapped` [`summary`](node_domain) constructor off the mapped
    /// VALUE's summary and folded into [`RaisedRootKind::Mapped`]'s
    /// `value_is_semantic_miss`, which the published-operator classifier
    /// ([`project_node_root_is_published_operator`]) consumes to suppress EXACTLY
    /// the carrier the `TypeExpr` predicate does (the miss spelling alone, NOT the
    /// broad sentinel set).
    pub(in crate::project_semantic_dispatch) root_semantic_miss_sentinel: bool,
    /// The NORMALIZED raised-ROOT term class — see [`RaisedRootKind`]. Set by the
    /// per-arm [`summary`](node_domain) constructors and carried up so the
    /// [`project_node_root_is_published_operator`] / [`project_node_root_is_typeof`]
    /// / [`project_node_root_is_object_surface`] / [`project_node_root_is_indexed_access`]
    /// classifiers answer off the POST-NORMALIZED root, matching the `TypeExpr`
    /// predicate on `raise(node)` by construction.
    pub(in crate::project_semantic_dispatch) root_kind: RaisedRootKind,
}

/// The NARROW root-only projection result — the FOUR root fields the root-only
/// classifier ([`node_domain::project_root_summary`]) genuinely computes, WITHOUT
/// the whole-tree `facts` AND. The root-only projection feeds the shared
/// [`summary`](node_domain) constructors PLACEHOLDER child facts (it never folds
/// member values), so the `RaisedShapeFacts` such a summary carries would be a LIE;
/// this type strips them and exposes ONLY the fields whose values MATCH THE FULL
/// FOLD's by construction (`root_kind` / `tag` / the two root sentinel flags).
#[derive(Debug, Clone, Copy)]
pub(in crate::project_semantic_dispatch) struct RootOnlySummary {
    pub(in crate::project_semantic_dispatch) root_kind: RaisedRootKind,
    pub(in crate::project_semantic_dispatch) tag: FactShapeTag,
    pub(in crate::project_semantic_dispatch) root_unmaterialized_sentinel: bool,
    pub(in crate::project_semantic_dispatch) root_semantic_miss_sentinel: bool,
}

impl RootOnlySummary {
    /// Narrow a shared [`summary`](node_domain)-layer [`RaisedShapeSummary`] to its
    /// VALID root-only fields, DROPPING the placeholder-fed `facts` the root-only
    /// projection never computes truthfully.
    pub(in crate::project_semantic_dispatch) fn from_summary(s: RaisedShapeSummary) -> Self {
        Self {
            root_kind: s.root_kind,
            tag: s.tag,
            root_unmaterialized_sentinel: s.root_unmaterialized_sentinel,
            root_semantic_miss_sentinel: s.root_semantic_miss_sentinel,
        }
    }
}

/// The bottom-up node-domain projection result: the interned structural key
/// plus the shared [`RaisedShapeSummary`] (facts + tag). The key-bearing
/// algebra wraps interning around the SAME summary the facts-only algebra
/// produces.
#[derive(Debug, Clone, Copy)]
pub(in crate::project_semantic_dispatch) struct RaisedShapeResult {
    pub(in crate::project_semantic_dispatch) key: RaisedShapeKey,
    pub(in crate::project_semantic_dispatch) summary: RaisedShapeSummary,
}

impl RaisedShapeResult {
    /// The raised-shape facts (the materialized / expanded-surface / shell-raise
    /// gate the Kind-B route helpers read).
    pub(in crate::project_semantic_dispatch) fn facts(&self) -> RaisedShapeFacts {
        self.summary.facts
    }
}

/// The combined facts + node-vs-`TypeExpr` equality result of ONE node fold: a
/// site needing BOTH the route-gate facts AND the no-op/changed decision reads
/// them from a single projection (one fold, one interner), instead of folding
/// the node twice. `eq_to_expr` is the node's raised shape compared against the
/// caller's input `&TypeExpr` interned in the SAME interner. `pub(crate)`
/// (consistent with [`RaisedShapeFacts`]) so the Kind-B sink adapters in
/// `resolver_core` read it through the `raise` re-export.
///
/// SEALED + node-bound: like [`RaisedNodeShapeFacts`], the FIELDS are PRIVATE and
/// the sole construction is the key-bearing fold ([`project_node_shape_for_eq`]),
/// which binds the queried `node`. A sibling cannot fabricate a
/// `NodeShapeEq { eq_to_expr: false, .. }` to force a `route_admission` "changed"
/// gate, and the gated `admit_expanded_surface_changed` mints from `shape.node()`
/// ALONE — so the carrier is bound to the SAME node whose shape produced the
/// equality decision. Cross-module readers use the getters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NodeShapeEq {
    node: SemanticNodeId,
    facts: RaisedShapeFacts,
    eq_to_expr: bool,
}

impl NodeShapeEq {
    /// The node this shape/equality was computed for — the ONLY admission input
    /// `admit_expanded_surface_changed` reads to bind the carrier it mints.
    #[must_use]
    pub(crate) fn node(&self) -> SemanticNodeId {
        self.node
    }
    /// The route-gate [`RaisedShapeFacts`] of the folded node.
    #[must_use]
    pub(crate) fn facts(&self) -> RaisedShapeFacts {
        self.facts
    }
    /// Whether the node's raised shape equals the caller's input `&TypeExpr`.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn eq_to_expr(&self) -> bool {
        self.eq_to_expr
    }
    /// `!eq_to_expr` — whether the node's raised shape DIFFERS from the caller's
    /// input `&TypeExpr` (the route "changed" gate's positive form).
    #[must_use]
    pub(crate) fn changed(&self) -> bool {
        !self.eq_to_expr
    }
}

// ===========================================================================
// The output algebra trait — parameterizes the ONE fold.
// ===========================================================================

/// Per-arm node construction for [`fold_node`]. The fold owns ALL control flow
/// (the `?` aborts, `filter_map` drops, the Intersection arm-drop + collapse,
/// the Object empty-vs-surface split, the carrier `<raise miss>` defaults, the
/// cycle guards); the algebra only constructs a node `Out` from already-folded
/// children + leaf data. The two compound arms that must INSPECT a folded child
/// (the Intersection drop rules) use [`Self::is_object_surface_sentinel`] /
/// [`Self::is_empty_object`].
trait RaisedShapeAlgebra {
    /// The folded value of a node.
    type Out;
    /// The algebra's function-signature representation: `Arc<FunctionExpr>` for
    /// the materializer, the interned `RaisedFunction` for the node-domain
    /// algebra. Extracted back out of a folded `Out` by [`Self::out_as_function`]
    /// for the `ConstructorType` rewrap and the Object method / call / construct
    /// signature members.
    type Fn;
    /// The algebra's object-member representation, accumulated by the surface
    /// helper and assembled by [`Self::object_from_members`].
    type Member;

    // -- Leaves --
    fn primitive(&mut self, kind: PrimitiveName) -> Self::Out;
    fn literal(&mut self, value: LiteralValue) -> Self::Out;
    fn infer(&mut self, name: Arc<str>) -> Self::Out;
    fn unknown(&mut self, raw: Arc<str>) -> Self::Out;
    /// A TYPED resolver-control sentinel reaching the reverse boundary (an
    /// alias / type-param cycle, a sub-result raise miss, an unrepresentable
    /// surface or surface member) — AND every
    /// other `Opaque(QueryError)` node (`Miss`, `Other(...)`, `BudgetExceeded`,
    /// `DeclPlaceholder`, …) reaching the `fold_node` `Opaque` conduit, since the
    /// input there is a typed `QueryError`, not a raw carrier. The materializer
    /// maps it through `semantic_query_error_raw` to the byte-identical legacy
    /// `Unknown { raw }` string; the node-domain algebras classify it directly
    /// from the typed [`QueryError`] via the shared sentinel authority — so a
    /// control sentinel never has to be spelled as a raw string to make a
    /// materialisation / tag decision. Distinct from [`Self::unknown`], which
    /// stays for strings that arrive RAW (the `RawFallback` carrier and
    /// externally-interned `Unknown` nodes).
    fn opaque_sentinel(&mut self, err: &QueryError) -> Self::Out;
    fn recursive_ref(&mut self, name: Arc<str>) -> Self::Out;
    /// A bare `Ref { name, type_arguments }` shell (also the `DeclPlaceholder`
    /// / `DeclRef` shell with empty args, and the `BareRef`/`InstantiationRef`
    /// carriers with raised-or-`<raise miss>` args).
    fn reference(&mut self, name: Arc<str>, type_arguments: Vec<Self::Out>) -> Self::Out;
    fn synthetic_slot_binding(
        &mut self,
        carrier: Arc<verter_type_expr::SyntheticCarrierKey>,
    ) -> Self::Out;
    fn import_type(
        &mut self,
        specifier: Arc<str>,
        qualifier: Arc<[Arc<str>]>,
        typeof_query: bool,
        type_arguments: Vec<Self::Out>,
    ) -> Self::Out;
    fn type_of(&mut self, path: Vec<String>, type_args: Vec<Self::Out>) -> Self::Out;

    // -- Compound --
    fn union(&mut self, members: Vec<Self::Out>) -> Self::Out;
    /// Build the intersection from its surviving (>=2) arms (the fold has
    /// already dropped sentinel/empty-object arms and handled the empty / single
    /// collapse cases).
    fn intersection(&mut self, arms: Vec<Self::Out>) -> Self::Out;
    /// The representable empty object `{}` (zero-property object).
    fn empty_object(&mut self) -> Self::Out;
    fn array(&mut self, element: Self::Out, readonly: bool) -> Self::Out;
    fn tuple(&mut self, elements: Vec<FoldedTupleElement<Self::Out>>, readonly: bool) -> Self::Out;
    fn key_of(&mut self, base: Self::Out) -> Self::Out;
    fn indexed_access(&mut self, object: Self::Out, index: Self::Out) -> Self::Out;
    fn conditional(
        &mut self,
        check: Self::Out,
        extends: Self::Out,
        true_type: Self::Out,
        false_type: Self::Out,
    ) -> Self::Out;
    #[allow(clippy::too_many_arguments)]
    fn mapped(
        &mut self,
        parameter: String,
        source: Self::Out,
        value: Self::Out,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<Self::Out>,
    ) -> Self::Out;
    fn template_literal(&mut self, quasis: Vec<String>, expressions: Vec<Self::Out>) -> Self::Out;
    fn type_parameter(
        &mut self,
        name: Arc<str>,
        constraint: Option<Self::Out>,
        default: Option<Self::Out>,
    ) -> Self::Out;

    // -- Function + ConstructorType --
    fn build_function(&mut self, function: FoldedFunction<Self::Out>) -> Self::Fn;
    fn function_to_out(&mut self, function: Self::Fn) -> Self::Out;
    fn constructor_to_out(&mut self, function: Self::Fn) -> Self::Out;
    /// Extract the function representation from a folded `Out` when it is a
    /// `Function` shape (the materializer matches `TypeExpr::Function`; the
    /// node-domain algebra matches the interned `Function` term). `None` for any
    /// other shape — used by the `ConstructorType` rewrap and the Object surface
    /// signature members.
    fn out_as_function(&self, out: &Self::Out) -> Option<Self::Fn>;

    // -- Object surface members --
    fn member_property(
        &mut self,
        name: String,
        ty: Self::Out,
        optional: bool,
        readonly: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    ) -> Self::Member;
    fn member_method(
        &mut self,
        name: String,
        function: Self::Fn,
        optional: bool,
        visibility: MemberVisibility,
        spans: verter_type_expr::MemberSpans,
    ) -> Self::Member;
    fn member_call_signature(&mut self, function: Self::Fn) -> Self::Member;
    fn member_construct_signature(&mut self, function: Self::Fn) -> Self::Member;
    fn member_index_signature(
        &mut self,
        key_name: String,
        key_type: Self::Out,
        value_type: Self::Out,
        readonly: bool,
        spans: verter_type_expr::IndexSignatureSpans,
    ) -> Self::Member;
    fn object_from_members(&mut self, members: Vec<Self::Member>) -> Self::Out;

    // -- Intersection arm-drop inspection (on a FOLDED child) --
    /// `true` when `out` is the `Unknown { raw == SEMANTIC_OBJECT_SURFACE }`
    /// sentinel (dropped from an intersection).
    fn is_object_surface_sentinel(&self, out: &Self::Out) -> bool;
    /// `true` when `out` is the representable empty object (`{} & X ≡ X`).
    fn is_empty_object(&self, out: &Self::Out) -> bool;
}

/// A folded tuple element awaiting algebra construction.
struct FoldedTupleElement<O> {
    label: Option<String>,
    ty: O,
    optional: bool,
    rest: bool,
}

/// A folded function shape awaiting algebra construction.
struct FoldedFunction<O> {
    parameters: Vec<FoldedFunctionParam<O>>,
    return_type: Option<O>,
    type_parameters: Vec<FoldedTypeParam<O>>,
    signature_span: Option<Span>,
    return_type_span: Option<Span>,
}

struct FoldedFunctionParam<O> {
    name: Option<Arc<str>>,
    ty: O,
    optional: bool,
    rest: bool,
    span: Option<Span>,
}

struct FoldedTypeParam<O> {
    name: Arc<str>,
    constraint: Option<O>,
    default: Option<O>,
}

// ===========================================================================
// The single shared fold.
// ===========================================================================

/// The SOLE exhaustive `SemanticNodeData` traversal: raise `node` one
/// structural level at a time, recursing children, applying every raiser
/// transform structurally, and building the algebra's `Out`. `None` when the
/// node — or a `?`-propagating required child — is unavailable / unraisable
/// from the live graph store.
///
/// Cycle protection via the per-call `active` visited set, guarded explicitly
/// ONLY at `Alias` + `TypeParam` (insert / early-return-sentinel / remove); the
/// `Object` arm uses a FRESH `active` per member. This control flow is the
/// historical `raise_node_to_type_expr_core_impl` body verbatim — the
/// byte-identity contract — re-housed so the materialization and the
/// node-domain facts/key share ONE traversal.
fn fold_node<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<A::Out> {
    let ctx = dispatch.ctx;
    let data = super::super::node_data_for(ctx, node)?;
    Some(match data.as_ref() {
        SemanticNodeData::Primitive(kind) => {
            alg.primitive(semantic_primitive_to_primitive_name(*kind))
        }
        SemanticNodeData::Literal(value) => alg.literal(value.clone()),
        SemanticNodeData::Alias(target) => {
            if !active.insert(node) {
                return Some(alg.opaque_sentinel(&QueryError::RaiseAliasCycle));
            }
            let result = fold_node(alg, dispatch, *target, active);
            active.remove(&node);
            return result;
        }
        SemanticNodeData::Union(members) => {
            let folded: Vec<A::Out> = members
                .iter()
                .filter_map(|member| fold_node(alg, dispatch, *member, active))
                .collect();
            alg.union(folded)
        }
        SemanticNodeData::Intersection(members) => {
            // filter_map recurse, then drop the SEMANTIC_OBJECT_SURFACE
            // sentinel arms and the empty-object arms (`{} & X ≡ X`), then
            // collapse: empty -> empty object, len==1 -> that arm, else
            // Intersection. The recurse is materialised into a Vec FIRST so the
            // arm-drop inspection (an immutable `alg` borrow) does not overlap
            // the recurse closure's unique `alg` borrow.
            let mut arms: Vec<A::Out> = members
                .iter()
                .filter_map(|member| fold_node(alg, dispatch, *member, active))
                .collect();
            arms.retain(|arm| !alg.is_object_surface_sentinel(arm) && !alg.is_empty_object(arm));
            if arms.is_empty() {
                alg.empty_object()
            } else if arms.len() == 1 {
                arms.into_iter().next().unwrap()
            } else {
                alg.intersection(arms)
            }
        }
        SemanticNodeData::Array { element, readonly } => {
            let element = fold_node(alg, dispatch, *element, active)?;
            alg.array(element, *readonly)
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            let folded: Vec<FoldedTupleElement<A::Out>> = elements
                .iter()
                .filter_map(|element| {
                    Some(FoldedTupleElement {
                        label: element
                            .label
                            .as_ref()
                            .map(|label| label.as_ref().to_string()),
                        ty: fold_node(alg, dispatch, element.value, active)?,
                        optional: element.optional,
                        rest: element.rest,
                    })
                })
                .collect();
            alg.tuple(folded, *readonly)
        }
        SemanticNodeData::Object(surface) => {
            if surface.members.is_empty()
                && surface.call_signatures.is_empty()
                && surface.construct_signatures.is_empty()
                && !surface.has_index_signature
            {
                alg.empty_object()
            } else {
                fold_surface_view(alg, dispatch, surface)
                    .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::UnrepresentableSurface))
            }
        }
        SemanticNodeData::MergedDecl { contributors } => {
            let merged =
                super::super::walk::reduce_merged_decl_with_graph(dispatch.graph(), contributors);
            return fold_node(alg, dispatch, merged, active);
        }
        SemanticNodeData::Opaque(QueryError::DeclPlaceholder { name, .. }) => {
            alg.reference(Arc::clone(name), Vec::new())
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => {
            let check = fold_node(alg, dispatch, *check, active)?;
            let extends = fold_node(alg, dispatch, *extends, active)?;
            let true_type = fold_node(alg, dispatch, *true_branch_ref, active)?;
            let false_type = fold_node(alg, dispatch, *false_branch_ref, active)?;
            alg.conditional(check, extends, true_type, false_type)
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => {
            let quasis: Vec<String> = quasis
                .iter()
                .map(|quasi| quasi.as_ref().to_string())
                .collect();
            let expressions: Vec<A::Out> = expressions
                .iter()
                .filter_map(|expr| fold_node(alg, dispatch, *expr, active))
                .collect();
            alg.template_literal(quasis, expressions)
        }
        SemanticNodeData::KeyOf { base } => {
            let base = fold_node(alg, dispatch, *base, active)?;
            alg.key_of(base)
        }
        SemanticNodeData::IndexedAccess { object, index } => {
            let object = fold_node(alg, dispatch, *object, active)?;
            let index = fold_index_key(alg, dispatch, index, active)?;
            alg.indexed_access(object, index)
        }
        SemanticNodeData::Mapped { mapper, .. } => {
            let parameter = match super::super::node_data_for(ctx, mapper.parameter_node).as_deref()
            {
                Some(SemanticNodeData::TypeParam { display_name, .. }) => {
                    display_name.as_ref().to_string()
                }
                _ => String::new(),
            };
            // The source recurses KeyOf-aware (matching the materializer's
            // explicit KeyOf shell around the mapped source key-space base).
            let source = match super::super::node_data_for(ctx, mapper.key_space)?.as_ref() {
                SemanticNodeData::KeyOf { base } => {
                    let base = fold_node(alg, dispatch, *base, active)?;
                    alg.key_of(base)
                }
                _ => fold_node(alg, dispatch, mapper.key_space, active)?,
            };
            let value = fold_node(alg, dispatch, mapper.value_expr, active)?;
            let optional = mapped_modifier_for_optionality(mapper.optionality);
            let readonly = mapped_modifier_for_readonly(mapper.readonly);
            let name_type = match mapper.name_remap {
                Some(remap) => Some(fold_node(alg, dispatch, remap, active)?),
                None => None,
            };
            alg.mapped(parameter, source, value, optional, readonly, name_type)
        }
        SemanticNodeData::TypeOf(_) => {
            let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
            let type_args = data.carrier_type_args();
            let mut segments = value_root
                .name
                .split('.')
                .map(|segment| segment.to_string())
                .collect::<Vec<_>>();
            segments.extend(path.iter().map(|segment| segment.as_ref().to_string()));
            let raised_args: Vec<A::Out> = type_args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active)
                        .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::RaiseMiss))
                })
                .collect();
            alg.type_of(segments, raised_args)
        }
        SemanticNodeData::TypeParam {
            display_name,
            constraint,
            default,
            ..
        } => {
            if !active.insert(node) {
                return Some(alg.opaque_sentinel(&QueryError::TypeParamCycle));
            }
            let constraint_out = constraint
                .as_ref()
                .and_then(|c| fold_node(alg, dispatch, *c, active));
            let default_out = default
                .as_ref()
                .and_then(|d| fold_node(alg, dispatch, *d, active));
            active.remove(&node);
            alg.type_parameter(Arc::clone(display_name), constraint_out, default_out)
        }
        SemanticNodeData::Infer { name } => alg.infer(Arc::clone(name)),
        SemanticNodeData::Opaque(err) => match err {
            QueryError::RecursiveRef { name } => alg.recursive_ref(Arc::clone(name)),
            // The input is a typed `QueryError`, not a raw carrier — route it
            // through the typed `opaque_sentinel` entry (BORROWED — no clone on
            // this hot traversal arm) instead of round-tripping it to a string and
            // re-deriving the materialised/tag facts. The materialize algebra still
            // emits the byte-identical `Unknown { raw: semantic_query_error_raw(err)
            // }`; the node-domain algebras classify directly from the typed
            // variant, held in agreement with the raw recogniser by the no-drift
            // contract.
            _ => alg.opaque_sentinel(err),
        },
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
            signature_span,
            return_type_span,
        } => {
            let folded = fold_function(
                alg,
                dispatch,
                params,
                *return_type,
                type_parameters,
                *signature_span,
                *return_type_span,
                active,
            );
            let function = alg.build_function(folded);
            alg.function_to_out(function)
        }
        SemanticNodeData::DeclRef { identity } => {
            alg.reference(Arc::clone(&identity.decl_name), Vec::new())
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            let raised_args: Vec<A::Out> = args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active)
                        .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::RaiseMiss))
                })
                .collect();
            alg.reference(Arc::clone(&base.decl_name), raised_args)
        }
        SemanticNodeData::BareRef(_) => {
            let (name, _scope) = data.bare_ref_head().expect("BareRef carrier head");
            let type_args = data.carrier_type_args();
            let raised_args: Vec<A::Out> = type_args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active)
                        .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::RaiseMiss))
                })
                .collect();
            alg.reference(Arc::clone(name), raised_args)
        }
        SemanticNodeData::ImportType(_) => {
            let (specifier, qualifier, typeof_query) =
                data.import_type_head().expect("ImportType carrier head");
            let type_args = data.carrier_type_args();
            let raised_args: Vec<A::Out> = type_args
                .iter()
                .map(|id| {
                    fold_node(alg, dispatch, *id, active)
                        .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::RaiseMiss))
                })
                .collect();
            alg.import_type(
                Arc::clone(specifier),
                Arc::clone(qualifier),
                typeof_query,
                raised_args,
            )
        }
        SemanticNodeData::RawFallback { raw } => alg.unknown(Arc::clone(raw)),
        SemanticNodeData::ConstructorType { signature } => {
            let raised = fold_node(alg, dispatch, *signature, active)?;
            match alg.out_as_function(&raised) {
                Some(function) => alg.constructor_to_out(function),
                None => raised,
            }
        }
        SemanticNodeData::SyntheticBinding { id, value_node } => {
            alg.synthetic_slot_binding(Arc::new(id.to_carrier_key(*value_node)))
        }
    })
}

/// Raise an [`IndexKey`] used as an `IndexedAccess` index — string / number
/// literals construct directly; a `TypeNode` recurses through the core.
fn fold_index_key<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    index: &IndexKey,
    active: &mut FxHashSet<SemanticNodeId>,
) -> Option<A::Out> {
    Some(match index {
        IndexKey::String(text) => alg.literal(LiteralValue::String(text.as_ref().to_string())),
        IndexKey::Number(number) => alg.literal(LiteralValue::Number(number.get() as f64)),
        IndexKey::TypeNode(node) => fold_node(alg, dispatch, *node, active)?,
    })
}

/// Fold a [`SemanticNodeData::Function`] payload into a [`FoldedFunction`].
#[allow(clippy::too_many_arguments)]
fn fold_function<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    params: &[crate::semantic_query::FunctionParam],
    return_type: SemanticNodeId,
    type_parameters: &[crate::semantic_query::TypeParamDecl],
    signature_span: Option<Span>,
    return_type_span: Option<Span>,
    active: &mut FxHashSet<SemanticNodeId>,
) -> FoldedFunction<A::Out> {
    let parameters: Vec<FoldedFunctionParam<A::Out>> = params
        .iter()
        .filter_map(|p| {
            Some(FoldedFunctionParam {
                name: p.name.clone(),
                ty: fold_node(alg, dispatch, p.ty, active)?,
                optional: p.optional,
                rest: p.rest,
                span: p.span,
            })
        })
        .collect();
    let return_out = fold_node(alg, dispatch, return_type, active);
    let type_params: Vec<FoldedTypeParam<A::Out>> = type_parameters
        .iter()
        .map(|tp| FoldedTypeParam {
            name: Arc::clone(&tp.name),
            constraint: tp
                .constraint
                .and_then(|c| fold_node(alg, dispatch, c, active)),
            default: tp.default.and_then(|d| fold_node(alg, dispatch, d, active)),
        })
        .collect();
    FoldedFunction {
        parameters,
        return_type: return_out,
        type_parameters: type_params,
        signature_span,
        return_type_span,
    }
}

/// Reconstruct an Object from a [`SurfaceView`] — the non-empty `Object` arm.
/// Each member / signature value folds through the core with a FRESH cycle set
/// (matching the materializer's fresh-per-member `active`). A member whose
/// value misses becomes the `SEMANTIC_SURFACE_MEMBER` sentinel. Returns `None`
/// when the surface yields no representable members (the empty-`{}` case is
/// handled by the caller).
fn fold_surface_view<A: RaisedShapeAlgebra>(
    alg: &mut A,
    dispatch: &ProjectSemanticDispatch<'_>,
    surface: &SurfaceView,
) -> Option<A::Out> {
    // Fold a member VALUE through the core with a fresh cycle set; a miss
    // becomes the SEMANTIC_SURFACE_MEMBER sentinel (matching the materializer).
    fn fold_member<A: RaisedShapeAlgebra>(
        alg: &mut A,
        dispatch: &ProjectSemanticDispatch<'_>,
        node: SemanticNodeId,
    ) -> A::Out {
        let mut active = FxHashSet::default();
        fold_node(alg, dispatch, node, &mut active)
            .unwrap_or_else(|| alg.opaque_sentinel(&QueryError::UnrepresentableSurfaceMember))
    }

    // Single-call-signature fast path: a surface with no members, no construct
    // signatures, no index signature, and exactly one call signature IS that
    // call signature's value (not wrapped in an object).
    if surface.members.is_empty()
        && surface.construct_signatures.is_empty()
        && !surface.has_index_signature
        && surface.call_signatures.len() == 1
    {
        return Some(fold_member(alg, dispatch, surface.call_signatures[0]));
    }

    let mut members: Vec<A::Member> = Vec::new();
    for member in surface.members.iter() {
        let ty = fold_member(alg, dispatch, member.value);
        if member.is_method {
            if let Some(function) = alg.out_as_function(&ty) {
                members.push(alg.member_method(
                    member.name.as_ref().to_string(),
                    function,
                    member.optional,
                    member.visibility,
                    member.spans,
                ));
                continue;
            }
        }
        members.push(alg.member_property(
            member.name.as_ref().to_string(),
            ty,
            member.optional,
            member.readonly,
            member.visibility,
            member.spans,
        ));
    }

    for signature in surface.call_signatures.iter() {
        let raised = fold_member(alg, dispatch, *signature);
        if let Some(function) = alg.out_as_function(&raised) {
            members.push(alg.member_call_signature(function));
        }
    }

    for signature in surface.construct_signatures.iter() {
        let raised = fold_member(alg, dispatch, *signature);
        if let Some(function) = alg.out_as_function(&raised) {
            members.push(alg.member_construct_signature(function));
        }
    }

    for signature in surface.index_signatures.iter() {
        let key_type = fold_member(alg, dispatch, signature.key_type);
        let value_type = fold_member(alg, dispatch, signature.value_type);
        members.push(alg.member_index_signature(
            "key".to_string(),
            key_type,
            value_type,
            signature.readonly,
            signature.spans,
        ));
    }

    // The synthetic open-surface placeholder ONLY when the surface is genuinely
    // OPEN (`has_index_signature` set, no concrete signature carried).
    if surface.has_index_signature && surface.index_signatures.is_empty() {
        let key_type = alg.primitive(PrimitiveName::String);
        let value_type = alg.unknown(Arc::from("projectedOpenSurface"));
        members.push(alg.member_index_signature(
            "key".to_string(),
            key_type,
            value_type,
            false,
            verter_type_expr::IndexSignatureSpans::default(),
        ));
    }

    if members.is_empty() {
        return None;
    }
    Some(alg.object_from_members(members))
}

// ===========================================================================
// Public entry points (pub(in crate::project_semantic_dispatch)) — the
// node-domain decision surface consumed by the `raise.rs` classifiers /
// equality primitives and the Kind-B callers.
// ===========================================================================

/// The node-bound [`RaisedNodeShapeFacts`] witness of `node` (its
/// [`RaisedShapeFacts`] PAIRED with `node`), or `None` when the whole raise is
/// `None`.
///
/// Folds through the FACTS-ONLY [`RaisedFactsAlg`] — it computes the SAME facts
/// as the key-bearing algebra (through the shared `node_domain::summary` layer)
/// WITHOUT interning a structural key, so a route gate that only reads
/// `materialized` / `expanded_surface` / `can_shell_raise` pays no key-DAG
/// construction. The witness binds the queried `node` to its own facts so a
/// `route_admission` mint helper can never pair them with a different node.
pub(in crate::project_semantic_dispatch) fn project_node_facts(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<RaisedNodeShapeFacts> {
    let mut alg = RaisedFactsAlg;
    let mut active = FxHashSet::default();
    let facts = fold_node(&mut alg, dispatch, node, &mut active)?.facts;
    Some(RaisedNodeShapeFacts { node, facts })
}

/// Project the wire-facing shallow named-member value from the SAME normalized
/// raised-shape fold used by output materialization. The decision stays in the
/// node domain; callers may materialize once afterwards solely for display.
pub(in crate::project_semantic_dispatch) fn project_node_shallow_member_output(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<RaisedShallowMemberOutput> {
    let mut interner = ShapeInterner::default();
    let result = {
        let mut alg = RaisedShapeAlg {
            interner: &mut interner,
        };
        let mut active = FxHashSet::default();
        fold_node(&mut alg, dispatch, node, &mut active)?
    };
    Some(match interner.term(result.key) {
        RaisedTerm::Primitive(name) => RaisedShallowMemberOutput::Primitive(*name),
        RaisedTerm::Literal(lit) => RaisedShallowMemberOutput::Literal(lit.clone()),
        RaisedTerm::Ref { name, .. } => RaisedShallowMemberOutput::Ref {
            name: Arc::clone(name),
        },
        RaisedTerm::Object(members) if members.is_empty() => RaisedShallowMemberOutput::EmptyObject,
        _ => RaisedShallowMemberOutput::Opaque,
    })
}

/// Whether `node`'s OWN raised root term is an unmaterialised sentinel — the
/// node-domain equivalent of `type_expr_root_is_unmaterialized_sentinel(raise(node))`.
/// Reads `root_unmaterialized_sentinel` off the ROOT-ONLY projection
/// ([`node_domain::project_root_summary`]) — a ROOT-term-only fact, so the
/// short-circuit projection is sufficient and matches the full fold's root term by
/// construction (no member-value walk, no `TypeExpr` materialised). `None` when the
/// whole raise is `None`.
pub(in crate::project_semantic_dispatch) fn project_node_root_sentinel(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<bool> {
    let mut active = FxHashSet::default();
    Some(
        node_domain::project_root_summary(dispatch, node, &mut active)?
            .root_unmaterialized_sentinel,
    )
}

/// The NORMALIZED raised-ROOT class of `node` — [`RaisedShapeSummary::root_kind`]
/// from the ROOT-ONLY projection ([`node_domain::project_root_summary`]), which
/// classifies the post-normalized root WITHOUT folding member values (an O(1)
/// shallow check for a large object surface, not the O(tree) whole-subtree walk
/// the full fold pays to also compute the here-unused `materialized` AND). Its
/// `root_kind` is pinned EQUAL to the full fold's by the parity test (the root
/// fields match the full fold; the placeholder-fed `facts` are stripped). `None`
/// when the whole raise is `None`. The per-fact classifiers below read this; a
/// caller never matches on the enum outside this module.
fn project_node_root_kind(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<RaisedRootKind> {
    let mut active = FxHashSet::default();
    Some(node_domain::project_root_summary(dispatch, node, &mut active)?.root_kind)
}

/// Node-domain equivalent of `type_expr_root_is_published_operator(raise(node))`:
/// whether `node`'s NORMALIZED raised root is a published surface operator — a
/// `Ref` / `KeyOf` / `IndexedAccess` / `Conditional` / `TypeOf`, OR a `Mapped`
/// whose value root is NOT the `semanticMiss` sentinel. Reads the post-normalized
/// [`RaisedRootKind`] off the ROOT-ONLY projection (no `TypeExpr` materialised),
/// so it answers IDENTICALLY to the `TypeExpr` predicate on
/// `raise(node)` even for shapes the raw-node mirror would mis-classify (e.g.
/// `Intersection([{}, IndexedAccess])`, which the root-only projection collapses
/// to its operator arm). `None` when the whole raise is `None`.
pub(in crate::project_semantic_dispatch) fn project_node_root_is_published_operator(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<bool> {
    Some(matches!(
        project_node_root_kind(dispatch, node)?,
        RaisedRootKind::Reference
            | RaisedRootKind::KeyOf
            | RaisedRootKind::IndexedAccess
            | RaisedRootKind::Conditional
            | RaisedRootKind::TypeOf
            | RaisedRootKind::Mapped {
                value_is_semantic_miss: false
            }
    ))
}

/// Whether `node`'s raised shape contains a semantic miss ANYWHERE in its tree —
/// the node-domain equivalent of `type_expr_contains_semantic_miss(raise(node))`,
/// expressed as `!RaisedShapeFacts.materialized` (the structural AND over all
/// value-bearing children; a single nested unresolved shell makes the whole
/// `materialized` fact `false`). `None` when the whole raise is `None`.
///
/// DISTINCT from [`project_node_root_sentinel`], which answers the ROOT-term-only
/// question (`type_expr_root_is_unmaterialized_sentinel`). A materialised object
/// whose nested member value carries a miss has `materialized == false` here but
/// `root_unmaterialized_sentinel == false` there — the two answer different
/// questions and must not be conflated.
///
/// Read through the `raise::node_contains_semantic_miss_with_dispatch` accessor
/// by the publication reducer's input-side no-poison gate.
pub(in crate::project_semantic_dispatch) fn project_node_contains_semantic_miss(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<bool> {
    project_node_facts(dispatch, node).map(|facts| !facts.materialized())
}

/// Combined facts + node-vs-`TypeExpr` equality of `node` in ONE fold: returns
/// the route-gate [`RaisedShapeFacts`] AND whether the node's raised shape
/// equals `expr`, computed from a SINGLE key-bearing fold (the input `&TypeExpr`
/// is interned into the SAME interner, never re-folded). `None` when the whole
/// raise is `None`. A site needing both — the changed/no-op gates — uses this
/// instead of folding the node twice (facts fold + equality fold).
pub(in crate::project_semantic_dispatch) fn project_node_shape_for_eq(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    expr: &TypeExpr,
) -> Option<NodeShapeEq> {
    let mut interner = ShapeInterner::default();
    let result = {
        let mut alg = RaisedShapeAlg {
            interner: &mut interner,
        };
        let mut active = FxHashSet::default();
        fold_node(&mut alg, dispatch, node, &mut active)?
    };
    let expr_key = type_expr_to_key(&mut interner, expr);
    Some(NodeShapeEq {
        node,
        facts: result.facts(),
        eq_to_expr: result.key == expr_key,
    })
}

/// Raised-shape equality of two nodes, computed in ONE shared interner so the
/// two keys are comparable. `Some(bool)` when BOTH raise to `Some`, `None` when
/// EITHER raise is `None`.
pub(in crate::project_semantic_dispatch) fn raised_shape_eq_nodes(
    dispatch: &ProjectSemanticDispatch<'_>,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> Option<bool> {
    let mut interner = ShapeInterner::default();
    let key_a = {
        let mut alg = RaisedShapeAlg {
            interner: &mut interner,
        };
        let mut active = FxHashSet::default();
        fold_node(&mut alg, dispatch, a, &mut active)?.key
    };
    let key_b = {
        let mut alg = RaisedShapeAlg {
            interner: &mut interner,
        };
        let mut active = FxHashSet::default();
        fold_node(&mut alg, dispatch, b, &mut active)?.key
    };
    Some(key_a == key_b)
}

/// Raised-shape equality of a node against a caller's `&TypeExpr`, computed in
/// ONE shared interner. `Some(bool)` when the node raises to `Some`, `None`
/// when the raise is `None`.
pub(in crate::project_semantic_dispatch) fn raised_shape_eq_node_type_expr(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    expr: &TypeExpr,
) -> Option<bool> {
    let mut interner = ShapeInterner::default();
    let node_key = {
        let mut alg = RaisedShapeAlg {
            interner: &mut interner,
        };
        let mut active = FxHashSet::default();
        fold_node(&mut alg, dispatch, node, &mut active)?.key
    };
    let expr_key = type_expr_to_key(&mut interner, expr);
    Some(node_key == expr_key)
}
