//! Prepared declaration structures consumed by the solver.
//!
//! Prepared declarations sit between `ShallowFileState`/frontier state and the
//! solver. They normalize declaration shape, build lookup tables, and classify
//! dependencies — but they do NOT perform full semantic solving.
//!
//! These structures are the long-term replacement for:
//! - `PreparedImportedTypeAlias`
//! - `PreparedLocalImportedTypeAlias`
//! - `PreparedImportedDeclContext`

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::host::ResolvedRootIdentity;
use crate::analysis::type_eval::{TypeDeclKind, ValueDeclKind};
use verter_type_expr::facts::{
    ClosednessRecipe, DeclarationOrigin, EnumMemberFact, FunctionSignatureFact, HeritageBaseFact,
    KeyDomainClosednessFact, NarrowTypeParam, ObjectShapeFact, PreparedCaseTransformKind,
    PreparedForwardPayloadFact, PreparedForwardingKind, PreparedKeyFilterShapeFact,
    PreparedKeyRemapShapeFact, PreparedMemberFact, PreparedProjectionClassFact,
    PreparedSurfaceModifiersFact, PreparedTypeBodyFacts, PreparedValueMemberFact,
    PreparedValueRuleShapeFact, PreparedWrapperKindFact, PreparedWrapperShapeFact, TypeBodyClass,
    ValueAnnotationClass, ValueTypeAnnotationFact, VueIgnoredHeritageFact,
};
use verter_type_expr::locators::{
    AuthoredAnchor, LocatorSymbolSpace, TypeArgLocator, TypeBodyPathStep, TypeBodySlot,
};
use verter_type_expr::span_origins::{DeclContributorAnchor, MemberSpansOrigin, SourceSynthetic};
use verter_type_expr::{MappedModifier, ObjectMember, PrimitiveName, TypeExpr};

/// The content-free anchor of a prepared declaration's authored positions.
fn decl_anchor(root_identity: &ResolvedRootIdentity, space: LocatorSymbolSpace) -> AuthoredAnchor {
    AuthoredAnchor {
        // The identity fields are shared `Arc<str>` — the anchor reuses the
        // same allocations instead of copying.
        canonical_id: Arc::clone(&root_identity.canonical_id),
        owner: root_identity.owner,
        symbol: Arc::clone(&root_identity.symbol_name),
        space,
    }
}

/// A body slot rooted at the declaration anchor with the given path.
fn decl_slot(
    root_identity: &ResolvedRootIdentity,
    space: LocatorSymbolSpace,
    path: Vec<TypeBodyPathStep>,
) -> TypeBodySlot {
    TypeBodySlot {
        anchor: decl_anchor(root_identity, space),
        path: path.into(),
    }
}

/// The "not a structural wrapper" classification — what every non-mapped or
/// unrecognized body keeps.
fn unclassified_wrapper_shape() -> PreparedWrapperShapeFact {
    PreparedWrapperShapeFact {
        kind: PreparedWrapperKindFact::None,
        source_param_index: None,
        key_filter: PreparedKeyFilterShapeFact::All,
        key_remap: PreparedKeyRemapShapeFact::Identity,
        value_rule: PreparedValueRuleShapeFact::PassThrough,
        modifiers: PreparedSurfaceModifiersFact {
            optional: None,
            readonly: None,
        },
    }
}

/// The span-recovery origin of one indexed member: descend `[ordinal]` from the
/// owning declaration's authored contributor statement, or an explicit
/// synthetic marker when the origin is not recoverable. Two miss cases:
///
/// - the body has no authored contributor anchor (genuinely synthetic), or
/// - the member was reached through an `IntersectionArm` descent (non-empty
///   `path_prefix`): `MemberSpansOrigin::Authored` member paths descend
///   top-level decl-body member ordinals only — an intersection-arm position
///   is not representable, so the honest typed miss is recorded instead of an
///   `Authored` origin claiming a position the schema cannot address.
fn member_span_origin(
    contributor: Option<DeclContributorAnchor>,
    ordinal: u32,
    path_prefix: &[TypeBodyPathStep],
) -> MemberSpansOrigin {
    match contributor {
        Some(anchor) if path_prefix.is_empty() => MemberSpansOrigin::Authored {
            anchor,
            member_path: Arc::from(vec![ordinal]),
        },
        _ => MemberSpansOrigin::Synthetic(SourceSynthetic),
    }
}

// ---------------------------------------------------------------------------
// Prepared type declaration
// ---------------------------------------------------------------------------

/// Shared empty `name_resolution` table used by the `new()` constructors, so
/// a freshly constructed prepared decl does not allocate a private empty map
/// (the prepared-decl builder assigns the real per-file shared table right
/// after construction; anchor-style consumers keep the empty table).
fn empty_name_resolution() -> Arc<FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>> {
    static EMPTY: std::sync::OnceLock<Arc<FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>>> =
        std::sync::OnceLock::new();
    Arc::clone(EMPTY.get_or_init(|| Arc::new(FxHashMap::default())))
}

/// Solver-facing prepared type declaration.
///
/// Prepared declarations are cache-owned, declaration-only, and intentionally
/// shallow: they carry classification FACTS plus content-free LOCATORS of the
/// authored body positions — never the body itself. The shared dispatch lowers
/// a located body on demand from the producing canonical's retained parse
/// snapshot.
///
/// Keyed by `(canonical_id, symbol_name, source_hash)` in the host cache.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct PreparedTypeDecl {
    /// Canonical identity of the defining file + symbol name.
    pub root_identity: ResolvedRootIdentity,

    /// The exported name (may differ from symbol_name due to aliasing).
    pub exported_name: Option<String>,

    /// Declaration kind.
    pub kind: TypeDeclKind,

    /// Generic type parameters — narrowed: constraint/default bounds are
    /// content-free locators of the authored bound positions, never embedded
    /// bodies.
    pub type_parameters: Vec<NarrowTypeParam>,

    /// Exact authored interface heritage arms suppressed only by Vue runtime
    /// props/emits projection. All ordinary semantic projections preserve
    /// these arms.
    pub vue_ignored_heritage: Arc<[VueIgnoredHeritageFact]>,

    /// The narrowed body FACTS: classification + the content-free body slot
    /// locator + ordered merged-contributor slots. The authored body is NOT
    /// stored — the shared dispatch lowers it on demand from the locator.
    /// A non-empty contributor-slot list marks a same-name merged interface
    /// (TS same-file declaration merging); body lowering interns a `MergedDecl`
    /// carrier over those contributors — preserving overload accumulation and
    /// member union under the peer-merge reducer (NOT the intersection
    /// heritage-shadow rule).
    pub body_facts: PreparedTypeBodyFacts,

    /// Member index for direct property/method lookup without walking the body.
    /// Populated for interfaces and object-like aliases. Default: empty.
    /// Each member is a narrowed fact: header flags + the content-free locator
    /// of its authored value position + its span-recovery origin.
    pub member_index: FxHashMap<String, PreparedMemberFact>,

    /// Same-file symbol references needed for local closure.
    pub local_deps: Vec<String>,

    /// Cross-file symbol references (canonical_id + name pairs).
    pub external_deps: Vec<PreparedExternalDep>,

    /// Pre-resolved name context: maps bare names appearing in the body
    /// to their resolved root identities. Built at prepare time from the
    /// defining file's local and import scope. Allows the solver to resolve
    /// cross-file references without going back to the host for route discovery.
    ///
    /// `Arc`-shared: the table is a per-FILE artifact for every
    /// non-namespaced declaration (file symbols + import bindings vary per
    /// file, not per declaration), so the prepared-decl builder shares ONE
    /// immutable base table across all such decls of a defining file; only a
    /// namespaced declaration (whose direct-sibling bindings are
    /// declaration-scoped) carries its own private table. Keys are interned
    /// `Arc<str>` names minted through the store-owned identity pool.
    pub name_resolution: Arc<FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>>,

    /// Declaration provenance metadata.
    pub provenance: DeclProvenance,

    /// Cache dependency contract for invalidation. Records the defining
    /// file hash, barrel/reexport participants, and local closure participants
    /// at preparation time. Used to check if this prepared entry is still valid.
    pub cache_deps: PreparedCacheDeps,

    /// Structural wrapper classification computed at preparation time.
    /// Enables the solver to fast-path identity wrappers, pure overlays,
    /// key filters, key remaps, and transparent aliases. Opaque filter/remap/
    /// transform payloads are content-free locators of the authored positions.
    pub wrapper_shape: PreparedWrapperShapeFact,

    /// Projection classification computed at preparation time.
    /// Determines how the solver can project individual members without
    /// fully instantiating the declaration body. Forward-subject type
    /// arguments are content-free locators of the authored argument positions.
    pub projection_class: PreparedProjectionClassFact,

    /// The producer-minted content-free heritage-base FACTS of a CLASS
    /// declaration body's Intersection fold (heritage `Ref` arms before the
    /// own `Object` arm): the authored base NAME (also the `name_resolution`
    /// routing key the dispatch head-resolution uses) plus one content-free
    /// [`TypeArgLocator`] per authored heritage type argument. Minted ONCE at
    /// lazy decl-body lowering by [`collect_heritage_base_facts`]; NEVER a
    /// resolved identity (heads resolve at dispatch time) and NEVER an
    /// embedded body (arguments deref + lower on demand). Empty for non-class
    /// declarations and heritage-free classes.
    pub heritage_bases: Arc<[HeritageBaseFact]>,

    /// The producer-minted per-declaration KEY-DOMAIN closedness fact —
    /// the closed-object SHAPE verdict plus one [`ClosednessRecipe`] per
    /// contributor body. Minted ONCE at lazy decl-body lowering by
    /// [`collect_key_domain_closedness_fact`] from the SAME transient bodies
    /// the fingerprint observes; the dispatch closedness evaluator reads it
    /// in place of any query-time authored-body `TypeExpr` walk. `None` for
    /// seeded (locator-only) states and enum groups — UNAVAILABLE, never a
    /// verdict.
    pub key_domain_closedness: Option<Arc<KeyDomainClosednessFact>>,
}

/// A cross-file dependency reference in a prepared declaration.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    verter_no_typeexpr::NoTypeExpr,
)]
pub struct PreparedExternalDep {
    pub canonical_id: String,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub symbol_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoredOrdinalOverflow {
    pub count: usize,
}

impl std::fmt::Display for AuthoredOrdinalOverflow {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "authored item count {} exceeds the u32 ordinal domain",
            self.count
        )
    }
}

impl std::error::Error for AuthoredOrdinalOverflow {}

#[cfg(test)]
mod prepared_external_dep_owner_tests {
    use super::PreparedExternalDep;
    use std::collections::HashSet;
    use verter_type_expr::TopLevelOwnerId;

    #[test]
    fn external_dependency_identity_discriminates_owner_in_memo_and_serde() {
        let make = |owner| PreparedExternalDep {
            canonical_id: "/src/dep.vue".to_string(),
            owner,
            symbol_name: "Shared".to_string(),
        };
        let module = make(TopLevelOwnerId::module(0));
        let instance = make(TopLevelOwnerId::instance(0));
        assert_ne!(module, instance);
        assert_eq!(HashSet::from([module.clone(), instance.clone()]).len(), 2);
        assert_ne!(
            serde_json::to_string(&module).unwrap(),
            serde_json::to_string(&instance).unwrap()
        );
        assert_eq!(
            serde_json::from_str::<PreparedExternalDep>(&serde_json::to_string(&module).unwrap())
                .unwrap(),
            module
        );
    }
}

/// Provenance metadata for a prepared declaration.
#[derive(Debug, Clone, Default, verter_no_typeexpr::NoTypeExpr)]
pub struct DeclProvenance {
    /// Route kind that resolved this declaration (direct, alias, wildcard).
    pub route_kind: Option<String>,
    /// Declaration-span ORIGIN locator: the authored top-level contributor
    /// statement (`program.body[contributor_index]`) whose span the producing
    /// canonical's retained parse snapshot recovers on demand (diagnostics
    /// display). Never a stored byte range.
    pub source_origin: Option<DeclContributorAnchor>,
    /// Barrel files traversed to reach the defining file.
    pub barrel_hops: Vec<String>,
}

/// Prepared declaration kind — broader than `TypeDeclKind` to support
/// declaration merging and enum dual-space treatment.
///
/// Not yet the primary `kind` field: `TypeDeclKind` remains the stored kind
/// until the session preparation surface adopts this broader taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreparedDeclKind {
    Alias,
    Interface,
    Class,
    Enum,
    Merged,
}

impl From<TypeDeclKind> for PreparedDeclKind {
    fn from(kind: TypeDeclKind) -> Self {
        match kind {
            TypeDeclKind::Alias => Self::Alias,
            TypeDeclKind::Interface => Self::Interface,
            TypeDeclKind::Class => Self::Class,
        }
    }
}

// ---------------------------------------------------------------------------
// Prepared value declaration
// ---------------------------------------------------------------------------

/// Solver-facing prepared value declaration.
///
/// Required for `typeof` without building an `EvalEnv`. Supports:
/// - `typeof x`
/// - dotted paths: `typeof ns.foo.bar`
/// - class/constructor/static-member queries
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct PreparedValueDecl {
    /// Canonical identity.
    pub root_identity: ResolvedRootIdentity,

    /// Exported name if different from the symbol name.
    pub exported_name: Option<String>,

    /// Value declaration kind.
    pub kind: ValueDeclKind,

    /// The narrowed annotation FACT: classification, the precomputed
    /// `typeof x` peel target, and the annotation source (an authored value
    /// annotation is its decl-body locator; an inferred one is host-raised).
    pub type_annotation: ValueTypeAnnotationFact,

    /// Narrowed function signature facts if the value is a function. Empty =
    /// non-callable; length 1 = the common single-declaration case; length > 1
    /// = an overload group (source order; the trailing entry may be the
    /// implementation, flagged by `has_implementation_body`). Return/parameter
    /// types are content-free locators of the authored positions.
    pub signatures: Vec<FunctionSignatureFact>,

    /// Narrowed object shape fact if the value is a const object / namespace.
    /// Member value types are content-free locators.
    pub object_shape: Option<ObjectShapeFact>,

    /// Member index for dotted path lookup (e.g. `typeof ns.member`) —
    /// narrowed value-member facts carrying body locators.
    pub member_index: FxHashMap<String, PreparedValueMemberFact>,

    /// For enum values: the full ordered narrowed member inventory, unioned
    /// across same-name merged enum contributors. Every member is present —
    /// foldable members carry their literal scalar, deferred members their
    /// degraded sound primitive domain — so `typeof Enum` / `Enum.Member` see
    /// EVERY member, never just the foldable subset.
    pub enum_members: Option<EnumMemberFact>,

    /// Cross-file dependencies.
    pub external_deps: Vec<PreparedExternalDep>,

    /// Pre-resolved name context for bare names in type annotations
    /// attached to this value declaration. Same semantics (and the same
    /// per-file `Arc` sharing + interned `Arc<str>` keys) as
    /// `PreparedTypeDecl::name_resolution`; the value-space table has no
    /// per-declaration bindings at all, so every prepared value decl of a
    /// defining file shares one base table.
    pub name_resolution: Arc<FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>>,

    /// Cache dependency contract for invalidation.
    pub cache_deps: PreparedCacheDeps,
}

/// Prepared value declaration kind — broader than `ValueDeclKind` to handle
/// enum objects and namespace objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PreparedValueDeclKind {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
    EnumObject,
    Namespace,
}

impl From<ValueDeclKind> for PreparedValueDeclKind {
    fn from(kind: ValueDeclKind) -> Self {
        match kind {
            ValueDeclKind::Const => Self::Const,
            ValueDeclKind::Let => Self::Let,
            ValueDeclKind::Var => Self::Var,
            ValueDeclKind::Function => Self::Function,
            ValueDeclKind::AsyncFunction => Self::AsyncFunction,
            ValueDeclKind::Class => Self::Class,
            ValueDeclKind::Enum => Self::EnumObject,
        }
    }
}

// ---------------------------------------------------------------------------
// Prepared cache dependency contract
// ---------------------------------------------------------------------------

/// Records the full dependency/provenance set used to build a prepared
/// declaration. Used for invalidation.
#[derive(Debug, Clone, Default, verter_no_typeexpr::NoTypeExpr)]
pub struct PreparedCacheDeps {
    /// Defining file canonical id + hash.
    pub defining_file: Option<(String, u64)>,
    /// Every barrel/reexport file that participated in route selection.
    pub barrel_participants: Vec<(String, u64)>,
    /// Local closure participant identities.
    pub local_closure_participants: Vec<String>,
}

impl PreparedCacheDeps {
    /// Check whether all recorded participants still match their cached version.
    pub fn is_valid(&self, current_hashes: &FxHashMap<String, u64>) -> bool {
        if let Some((ref id, hash)) = self.defining_file {
            if current_hashes.get(id.as_str()).copied() != Some(hash) {
                return false;
            }
        }
        for (id, hash) in &self.barrel_participants {
            if current_hashes.get(id.as_str()).copied() != Some(*hash) {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// Builder helpers
// ---------------------------------------------------------------------------

impl PreparedTypeDecl {
    /// Create a new prepared type declaration with minimal fields. The body
    /// FACTS are minted from the declaration's own anchor (classification from
    /// `kind`; the body slot addresses the whole authored body); extra fields
    /// (member_index, deps, provenance) are defaulted. The producer then feeds
    /// the TRANSIENT authored body to [`build_member_index`](Self::build_member_index)
    /// / [`classify_wrapper_shape`](Self::classify_wrapper_shape) /
    /// [`classify_projection`](Self::classify_projection) — the body itself is
    /// never retained.
    pub fn new(root_identity: ResolvedRootIdentity, kind: TypeDeclKind) -> Self {
        let body_facts = PreparedTypeBodyFacts {
            classification: match kind {
                TypeDeclKind::Alias => TypeBodyClass::Alias,
                TypeDeclKind::Interface => TypeBodyClass::Interface,
                TypeDeclKind::Class => TypeBodyClass::Class,
            },
            body_slot: decl_slot(&root_identity, LocatorSymbolSpace::Type, Vec::new()),
            merged_contributor_slots: Arc::from([]),
        };
        Self {
            root_identity,
            exported_name: None,
            kind,
            type_parameters: Vec::new(),
            vue_ignored_heritage: Arc::from([]),
            body_facts,
            member_index: FxHashMap::default(),
            local_deps: Vec::new(),
            external_deps: Vec::new(),
            name_resolution: empty_name_resolution(),
            provenance: DeclProvenance::default(),
            cache_deps: PreparedCacheDeps::default(),
            wrapper_shape: unclassified_wrapper_shape(),
            projection_class: PreparedProjectionClassFact::Opaque,
            heritage_bases: Arc::from([]),
            key_domain_closedness: None,
        }
    }

    /// Record that this declaration is a same-name merged interface with
    /// `count` ordered contributors: mints one contributor slot per ordinal
    /// and flips the body classification to
    /// [`TypeBodyClass::MergedInterface`]. A zero count resets to the
    /// non-merged state.
    pub fn set_merged_contributors(&mut self, count: usize) -> Result<(), AuthoredOrdinalOverflow> {
        if count == 0 {
            self.body_facts.merged_contributor_slots = Arc::from([]);
            self.body_facts.classification = match self.kind {
                TypeDeclKind::Alias => TypeBodyClass::Alias,
                TypeDeclKind::Interface => TypeBodyClass::Interface,
                TypeDeclKind::Class => TypeBodyClass::Class,
            };
            return Ok(());
        }
        u32::try_from(count).map_err(|_| AuthoredOrdinalOverflow { count })?;
        let Some(slots) = (0..count)
            .map(|ordinal| {
                let ordinal = u32::try_from(ordinal).ok()?;
                Some(decl_slot(
                    &self.root_identity,
                    LocatorSymbolSpace::Type,
                    vec![TypeBodyPathStep::MergedContributor { ordinal }],
                ))
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Err(AuthoredOrdinalOverflow { count });
        };
        self.body_facts.merged_contributor_slots = slots.into();
        self.body_facts.classification = TypeBodyClass::MergedInterface;
        Ok(())
    }

    /// Build a member index from the TRANSIENT object-like authored body.
    ///
    /// Handles:
    /// - `TypeExpr::Object` — direct properties
    /// - `TypeExpr::Intersection` — scan parts right-to-left, indexing direct
    ///   object members. Right-to-left precedence ensures the interface's own
    ///   object tail (last part) wins over inherited parts (earlier parts).
    ///   Only direct Object members are indexed; heritage Ref parts are skipped.
    ///   Nested transparent intersections are descended so declaration-merged
    ///   interfaces still expose members from earlier object slices.
    ///
    /// Each indexed member's `ty` is the content-free LOCATOR of its authored
    /// value position — the RAW body path the shared deref navigates:
    /// `[IntersectionArm { arm } ..., Member { ordinal }, MemberValue]`, where
    /// each `IntersectionArm` step carries the arm's raw source index at its
    /// intersection level (parenthesized layers are structurally transparent
    /// and take no step) and `Member.ordinal` is the raw index into the
    /// containing object's `properties` — counting nameless call / construct /
    /// index signatures. The member-index MAP stays name-keyed (the lookup
    /// key); only the minted locator carries the raw body path.
    ///
    /// `contributor` is the owning declaration's authored top-level statement
    /// anchor: `Some(anchor)` mints each TOP-LEVEL object member's
    /// span-recovery origin as `[ordinal]` (the raw index within its
    /// containing object's authored member surface) descended from it; `None`
    /// asserts the body is GENUINELY synthetic (hand-built fixtures /
    /// synthesized surfaces) and records explicit `Synthetic` origins. Passing
    /// `None` for an authored body is forbidden — a synthetic origin must
    /// never stand in for an authored member whose anchor the producer failed
    /// to thread through. A member reached through an `IntersectionArm`
    /// descent records a `Synthetic` origin even under `Some(anchor)`:
    /// `MemberSpansOrigin::Authored` member paths descend top-level decl-body
    /// member ordinals only, so an intersection-arm position is not
    /// representable — the explicit typed miss, never a dishonest `Authored`.
    pub fn build_member_index(
        &mut self,
        body: &TypeExpr,
        contributor: Option<DeclContributorAnchor>,
    ) {
        // The defining file of this declaration is the declaration site of
        // every own-body member it indexes (heritage Ref parts are skipped),
        // so each member fact is stamped with it. The macro-surface overlay
        // pairs the member's recovered spans with this file.
        let declaration_origin =
            DeclarationOrigin::Declared(Arc::clone(&self.root_identity.canonical_id));
        let mut path_prefix = Vec::new();
        Self::index_transparent_object_members(
            &mut self.member_index,
            body,
            &declaration_origin,
            contributor,
            &self.root_identity,
            &mut path_prefix,
        );
    }

    /// Index direct object members into the member_index map.
    /// Existing entries are NOT overwritten (preserves right-to-left precedence
    /// when called from intersection traversal).
    ///
    /// `path_prefix` is the raw body path from the decl body root to this
    /// object (`IntersectionArm` steps only; empty for an object-root body);
    /// each member's locator appends `[Member { raw_index }, MemberValue]`.
    fn index_object_members(
        member_index: &mut rustc_hash::FxHashMap<String, PreparedMemberFact>,
        obj: &verter_type_expr::ObjectExpr,
        declaration_origin: &DeclarationOrigin,
        contributor: Option<DeclContributorAnchor>,
        root_identity: &ResolvedRootIdentity,
        path_prefix: &[TypeBodyPathStep],
    ) {
        for (raw_index, member) in obj.properties.iter().enumerate() {
            // The RAW index into this object's `properties` — the exact index
            // the `Member` step derefs. Nameless call / construct / index
            // signatures occupy their positions, so a named member after one
            // does NOT compact down.
            let Ok(ordinal) = u32::try_from(raw_index) else {
                break;
            };
            let member_value_path = || {
                let mut path = path_prefix.to_vec();
                path.push(TypeBodyPathStep::Member { ordinal });
                path.push(TypeBodyPathStep::MemberValue);
                path
            };
            match member {
                ObjectMember::Property(prop) => {
                    // entry API: only insert if not already present
                    member_index
                        .entry(prop.name.clone())
                        .or_insert_with(|| PreparedMemberFact {
                            optional: prop.optional,
                            readonly: prop.readonly,
                            is_method: false,
                            // Carry the IR property's declared accessibility.
                            visibility: prop.visibility,
                            // Stamp this declaration's defining file.
                            declaration_origin: declaration_origin.clone(),
                            ty: decl_slot(
                                root_identity,
                                LocatorSymbolSpace::Type,
                                member_value_path(),
                            ),
                            // Span-recovery origin: the member's raw index
                            // within its containing object's authored member
                            // surface, under the owning declaration's
                            // contributor anchor — recoverable only for a
                            // top-level object member; an intersection-arm
                            // member records the typed miss.
                            span_origin: member_span_origin(contributor, ordinal, path_prefix),
                        });
                }
                ObjectMember::Method(method) => {
                    // Own method members are also direct own-body members —
                    // index them so the macro-surface own-member overlay can
                    // stamp `declared_in_macro_type_arg` for an own interface
                    // method. The locator addresses the method's value
                    // surface; the dispatch lowers its function shape on
                    // demand.
                    member_index
                        .entry(method.name.clone())
                        .or_insert_with(|| PreparedMemberFact {
                            optional: method.optional,
                            readonly: false,
                            is_method: true,
                            // Carry the IR method's declared accessibility.
                            visibility: method.visibility,
                            // Stamp this declaration's defining file.
                            declaration_origin: declaration_origin.clone(),
                            ty: decl_slot(
                                root_identity,
                                LocatorSymbolSpace::Type,
                                member_value_path(),
                            ),
                            span_origin: member_span_origin(contributor, ordinal, path_prefix),
                        });
                }
                _ => {}
            }
        }
    }

    /// Descend the transparent structure of an object-like body, indexing each
    /// direct object's members. `path_prefix` accumulates the raw body path to
    /// the current position: an `IntersectionArm { ordinal }` step per
    /// intersection level (the arm's raw source index — NOT its reversed visit
    /// order); a parenthesized layer is structurally transparent to the deref
    /// and takes no step.
    fn index_transparent_object_members(
        member_index: &mut rustc_hash::FxHashMap<String, PreparedMemberFact>,
        body: &TypeExpr,
        declaration_origin: &DeclarationOrigin,
        contributor: Option<DeclContributorAnchor>,
        root_identity: &ResolvedRootIdentity,
        path_prefix: &mut Vec<TypeBodyPathStep>,
    ) {
        match body {
            TypeExpr::Object(obj) => Self::index_object_members(
                member_index,
                obj,
                declaration_origin,
                contributor,
                root_identity,
                path_prefix,
            ),
            TypeExpr::Intersection(parts) => {
                // Right-to-left visit order (last arm wins the name-keyed
                // entry); each arm's PATH step carries its raw source index.
                for (arm_index, part) in parts.iter().enumerate().rev() {
                    let Ok(ordinal) = u32::try_from(arm_index) else {
                        continue;
                    };
                    path_prefix.push(TypeBodyPathStep::IntersectionArm { ordinal });
                    Self::index_transparent_object_members(
                        member_index,
                        part,
                        declaration_origin,
                        contributor,
                        root_identity,
                        path_prefix,
                    );
                    path_prefix.pop();
                }
            }
            TypeExpr::Parenthesized(inner) => {
                Self::index_transparent_object_members(
                    member_index,
                    inner,
                    declaration_origin,
                    contributor,
                    root_identity,
                    path_prefix,
                );
            }
            _ => {}
        }
    }

    /// Look up a member by name. O(1) if the member index is populated.
    pub fn member(&self, name: &str) -> Option<&PreparedMemberFact> {
        self.member_index.get(name)
    }

    /// Classify using the broader PreparedDeclKind.
    pub fn prepared_kind(&self) -> PreparedDeclKind {
        PreparedDeclKind::from(self.kind)
    }

    /// Classify the structural wrapper shape from the TRANSIENT authored body
    /// and type parameters.
    ///
    /// Must be called after `type_parameters` is populated. Sets `self.wrapper_shape`.
    pub fn classify_wrapper_shape(&mut self, body: &TypeExpr) {
        self.wrapper_shape =
            classify_wrapper_shape_inner(body, &self.type_parameters, &self.root_identity);
    }

    /// Classify the projection class from the TRANSIENT authored body, member
    /// index, and wrapper shape.
    ///
    /// Must be called after `build_member_index()` and `classify_wrapper_shape()`.
    /// Sets `self.projection_class`.
    pub fn classify_projection(&mut self, body: &TypeExpr) {
        self.projection_class = classify_projection_inner(
            body,
            &self.type_parameters,
            &self.member_index,
            &self.wrapper_shape,
            &self.root_identity,
        );
    }
}

/// Extract the content-free heritage-base FACTS from ONE transient authored
/// CLASS contributor body — a pure syntactic extraction over the producer's
/// Intersection fold (heritage `Ref` arms before the own `Object` arm).
///
/// For each DIRECT `Ref` arm of the top-level `Intersection`, mints one
/// [`HeritageBaseFact`]: the authored base name (also the `name_resolution`
/// routing key) plus one [`TypeArgLocator`] per authored type argument, whose
/// path is `path_prefix ++ [IntersectionArm { arm ordinal }]` rooted at the
/// declaration's type-space anchor (`path_prefix` carries the
/// `MergedContributor` step for a merged group's contributor; empty for a
/// single body). The extraction does NOT resolve the base (head resolution is
/// the dispatch's job) and does NOT lower the arguments (they deref + lower on
/// demand). A non-`Intersection` body (a heritage-free class) yields no facts.
///
/// The caller gates on the declaration KIND: only a CLASS body's Intersection
/// fold encodes heritage — an interface's extends fold serves the instance
/// rail structurally and an alias's authored intersection is composition, not
/// heritage.
///
/// The base-name span is not representable in the member-ordinal
/// [`MemberSpansOrigin::Authored`] vocabulary (heritage arms are not object
/// members), so each fact records the explicit
/// [`MemberSpansOrigin::Synthetic`] typed miss — never a dishonest `Authored`
/// origin (the same rule [`PreparedTypeDecl::build_member_index`] applies to
/// intersection-arm member positions).
pub fn collect_heritage_base_facts(
    root_identity: &ResolvedRootIdentity,
    body: &TypeExpr,
    path_prefix: &[TypeBodyPathStep],
) -> Vec<HeritageBaseFact> {
    let TypeExpr::Intersection(parts) = body else {
        return Vec::new();
    };
    let anchor = decl_anchor(root_identity, LocatorSymbolSpace::Type);
    parts
        .iter()
        .enumerate()
        .filter_map(|(arm, part)| {
            let ordinal = u32::try_from(arm).ok()?;
            let TypeExpr::Ref {
                name,
                type_arguments,
            } = part
            else {
                return None;
            };
            let mut path: Vec<TypeBodyPathStep> = Vec::with_capacity(path_prefix.len() + 1);
            path.extend_from_slice(path_prefix);
            path.push(TypeBodyPathStep::IntersectionArm { ordinal });
            let path: Arc<[TypeBodyPathStep]> = path.into();
            let type_args: Arc<[TypeArgLocator]> = (0..type_arguments.len())
                .map(|arg_index| {
                    Some(TypeArgLocator {
                        anchor: anchor.clone(),
                        path: Arc::clone(&path),
                        arg_index: u32::try_from(arg_index).ok()?,
                    })
                })
                .collect::<Option<Vec<_>>>()?
                .into();
            Some(HeritageBaseFact {
                name: name.to_string(),
                type_args,
                name_resolution_ref: name.to_string(),
                base_name_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
            })
        })
        .collect()
}

/// Whether ONE transient authored contributor body is a closed-object SHAPE —
/// an `Object`, an intersection of closed-object shapes, or a parenthesized
/// chain of those. The nominal-interface carve-out verdict the publication
/// terminals consult, minted at lazy decl-body lowering (pure syntax: index
/// signatures and member values are NOT consulted — a nominal object surface
/// stays a carrier regardless of its values; a union is NOT a closed-object
/// shape).
pub fn body_is_closed_object_shape(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Object(_) => true,
        TypeExpr::Intersection(arms) => arms.iter().all(body_is_closed_object_shape),
        TypeExpr::Parenthesized(inner) => body_is_closed_object_shape(inner),
        _ => false,
    }
}

/// Extract the content-free KEY-DOMAIN closedness fact from the transient
/// authored contributor bodies of ONE type declaration group — a pure
/// syntactic extraction (no name resolution, no lowering, no verdicts beyond
/// binding-independent-sound shapes; everything else escapes by locator to
/// the dispatch-time node-route classifier).
///
/// `merged` mirrors the group's `TypeDeclBody` merge shape: a merged group
/// mints per contributor under its `MergedContributor` path step (the same
/// ordinal space the locator deref's transient shape serves); a single group
/// mints from the primary (last-wins) body with an empty prefix — the one
/// body the whole-body locator deref serves.
pub fn collect_key_domain_closedness_fact(
    root_identity: &ResolvedRootIdentity,
    bodies: &[TypeExpr],
    merged: bool,
) -> KeyDomainClosednessFact {
    let recipes: Vec<ClosednessRecipe> = if merged {
        bodies
            .iter()
            .enumerate()
            .filter_map(|(ordinal, body)| {
                let mut path = vec![TypeBodyPathStep::MergedContributor {
                    ordinal: u32::try_from(ordinal).ok()?,
                }];
                Some(closedness_recipe_of(root_identity, body, &mut path))
            })
            .collect()
    } else {
        bodies
            .last()
            .map(|body| {
                let mut path = Vec::new();
                closedness_recipe_of(root_identity, body, &mut path)
            })
            .into_iter()
            .collect()
    };
    // The closed-object SHAPE verdict folds over the SAME body set the
    // recipes cover (merged: every contributor; single: the primary body) —
    // the exact transient set the query-time walk previously consumed.
    let shape_bodies: &[TypeExpr] = if merged {
        bodies
    } else {
        bodies.last().map(std::slice::from_ref).unwrap_or(&[])
    };
    KeyDomainClosednessFact {
        closed_object_shape: !shape_bodies.is_empty()
            && shape_bodies.iter().all(body_is_closed_object_shape),
        body_recipes: recipes.into(),
    }
}

/// One body position's closedness recipe. `path` is the locator path TO this
/// position (rooted at the declaration's type-space anchor); it grows only
/// through the composition arm (union / intersection ordinals) — every other
/// complex shape escapes AT its position with the accumulated path.
fn closedness_recipe_of(
    root_identity: &ResolvedRootIdentity,
    expr: &TypeExpr,
    path: &mut Vec<TypeBodyPathStep>,
) -> ClosednessRecipe {
    match expr {
        // Parentheses are transparent to both the recipe semantics and the
        // locator navigation (which unwraps them at every expression step),
        // so they mint NO arm and consume NO path step.
        TypeExpr::Parenthesized(inner) => closedness_recipe_of(root_identity, inner, path),
        TypeExpr::Literal(_) | TypeExpr::Primitive(_) => ClosednessRecipe::ClosedLeaf,
        // An object's NAMED members fix its key domain regardless of member
        // values — but an index-signature KEY that is not a syntactically
        // closed scalar needs the full walker (it may be a bound parameter,
        // a closed ref, or an open interpolation): escape the whole object.
        TypeExpr::Object(obj) => {
            let keys_scalar = obj.properties.iter().all(|member| match member {
                ObjectMember::IndexSignature(sig) => scalar_key_shape(&sig.key_type),
                _ => true,
            });
            if keys_scalar {
                ClosednessRecipe::ObjectClosed
            } else {
                ClosednessRecipe::LowerAndClassify {
                    slot: decl_slot(root_identity, LocatorSymbolSpace::Type, path.clone()),
                }
            }
        }
        TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => ClosednessRecipe::OpenLeaf,
        TypeExpr::Union(arms) => collect_all_arms(root_identity, arms, path, |ordinal| {
            TypeBodyPathStep::UnionArm { ordinal }
        }),
        TypeExpr::Intersection(arms) => collect_all_arms(root_identity, arms, path, |ordinal| {
            TypeBodyPathStep::IntersectionArm { ordinal }
        }),
        TypeExpr::TypeParameter(param) => ClosednessRecipe::ParamRef {
            name: param.name.clone(),
        },
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => ClosednessRecipe::FollowRefByName {
            name: name.to_string(),
        },
        // An indexed access is judged OPERAND-WISE: the object operand is
        // VALUE-SENSITIVE, the index a key/keyspace question — a
        // whole-position escape would let the lowerer execute a literal
        // access and lose the value-sensitive operand rule.
        TypeExpr::IndexedAccess { .. } => {
            let mut object_path = path.clone();
            object_path.push(TypeBodyPathStep::IndexedAccessObject);
            let mut index_path = path.clone();
            index_path.push(TypeBodyPathStep::IndexedAccessIndex);
            ClosednessRecipe::ValueProjection {
                object: decl_slot(root_identity, LocatorSymbolSpace::Type, object_path),
                index: decl_slot(root_identity, LocatorSymbolSpace::Type, index_path),
            }
        }
        // `typeof x` value queries, recursion placeholders, synthetic
        // carriers, and unlowerable fragments cannot be classified from
        // syntax or the node route — UNAVAILABLE, never a false verdict.
        TypeExpr::TypeOf(_)
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::SyntheticSlotBinding(_)
        | TypeExpr::Unknown { .. } => ClosednessRecipe::Unsupported,
        // Everything else — generic/builtin instantiations, conditionals,
        // mapped/indexed/keyof/template operators, tuples, arrays, rests,
        // infer placeholders, import-type carriers — escapes to the
        // dispatch-time node-route classifier at this position.
        _ => ClosednessRecipe::LowerAndClassify {
            slot: decl_slot(root_identity, LocatorSymbolSpace::Type, path.clone()),
        },
    }
}

/// The composition fold shared by the union / intersection arms.
fn collect_all_arms(
    root_identity: &ResolvedRootIdentity,
    arms: &[TypeExpr],
    path: &mut Vec<TypeBodyPathStep>,
    step: impl Fn(u32) -> TypeBodyPathStep,
) -> ClosednessRecipe {
    let recipes: Vec<ClosednessRecipe> = arms
        .iter()
        .enumerate()
        .filter_map(|(ordinal, arm)| {
            path.push(step(u32::try_from(ordinal).ok()?));
            let recipe = closedness_recipe_of(root_identity, arm, path);
            path.pop();
            Some(recipe)
        })
        .collect();
    ClosednessRecipe::AllArms(recipes.into())
}

/// Whether an index-signature KEY is a syntactically closed scalar (a literal
/// / primitive, through parentheses) — the only key shapes [`ClosednessRecipe::ObjectClosed`]
/// may absorb without the walker.
fn scalar_key_shape(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Literal(_) | TypeExpr::Primitive(_) => true,
        TypeExpr::Parenthesized(inner) => scalar_key_shape(inner),
        _ => false,
    }
}

/// Check if a TypeExpr is a bare `Ref` to the given name with no type args.
fn is_bare_ref(expr: &TypeExpr, name: &str) -> bool {
    matches!(expr, TypeExpr::Ref { name: n, type_arguments } if &**n == name && type_arguments.is_empty())
}

/// Check if a TypeExpr is `T[K]` (indexed access of base by param).
fn is_passthrough_value(value: &TypeExpr, base_name: &str, param_name: &str) -> bool {
    match value {
        TypeExpr::IndexedAccess { object, index } => {
            is_bare_ref(object, base_name) && is_bare_ref(index, param_name)
        }
        _ => false,
    }
}

/// Classify the body of a mapped type declaration into a
/// `PreparedWrapperShapeFact`. Non-literal remap / transform payloads become
/// content-free LOCATORS of the authored mapped positions (the body root IS
/// the mapped type, so the paths are `[MappedNameType]` / `[MappedValue]`).
fn classify_wrapper_shape_inner(
    body: &TypeExpr,
    type_params: &[NarrowTypeParam],
    root_identity: &ResolvedRootIdentity,
) -> PreparedWrapperShapeFact {
    // Only classify mapped type bodies with at least one type param
    let (param, source, value, optional, readonly, name_type) = match body {
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => (parameter, source, value, optional, readonly, name_type),
        _ => {
            // Non-mapped body — not a structural wrapper. Alias forwarding
            // is handled by PreparedProjectionClassFact::ForwardSubject instead.
            return unclassified_wrapper_shape();
        }
    };

    // Source must be `keyof T` where T is a type parameter
    let base_param = match &**source {
        TypeExpr::KeyOf(inner) => match &**inner {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => type_params
                .iter()
                .position(|tp| tp.name == **name)
                .map(|idx| (idx, &**name)),
            _ => None,
        },
        _ => None,
    };

    let (source_param_index, base_name) = match base_param {
        Some((idx, name)) => (idx, name),
        None => return unclassified_wrapper_shape(),
    };

    // Classify optional/readonly modifiers
    let opt_mod = match optional {
        MappedModifier::None => None,
        MappedModifier::Add => Some(true),
        MappedModifier::Remove => Some(false),
    };
    let ro_mod = match readonly {
        MappedModifier::None => None,
        MappedModifier::Add => Some(true),
        MappedModifier::Remove => Some(false),
    };
    let modifiers = PreparedSurfaceModifiersFact {
        optional: opt_mod,
        readonly: ro_mod,
    };

    // Check value rule: is it `T[K]` (passthrough)? A transform is the LOCATOR
    // of the authored mapped-value position.
    let value_rule = if is_passthrough_value(value, base_name, param) {
        PreparedValueRuleShapeFact::PassThrough
    } else {
        PreparedValueRuleShapeFact::Transform(decl_slot(
            root_identity,
            LocatorSymbolSpace::Type,
            vec![TypeBodyPathStep::MappedValue],
        ))
    };

    // Check name_type for key remap
    let key_remap = match name_type {
        None => PreparedKeyRemapShapeFact::Identity,
        Some(nt) => classify_key_remap(nt, param, root_identity),
    };

    // Determine the kind
    let is_passthrough = matches!(value_rule, PreparedValueRuleShapeFact::PassThrough);
    let is_identity_remap = matches!(key_remap, PreparedKeyRemapShapeFact::Identity);

    let kind = if is_passthrough && is_identity_remap && opt_mod.is_none() && ro_mod.is_none() {
        PreparedWrapperKindFact::Identity
    } else if is_passthrough && is_identity_remap {
        PreparedWrapperKindFact::PureOverlay
    } else if !is_identity_remap {
        PreparedWrapperKindFact::KeyRemap
    } else {
        PreparedWrapperKindFact::None
    };

    PreparedWrapperShapeFact {
        kind,
        source_param_index: Some(source_param_index as u16),
        key_filter: PreparedKeyFilterShapeFact::All,
        key_remap,
        value_rule,
        modifiers,
    }
}

/// Classify key remap from a name_type expression. A non-literal remap is the
/// content-free LOCATOR of the authored `as`-clause position (`[MappedNameType]`
/// from the decl body root).
fn classify_key_remap(
    name_type: &TypeExpr,
    param: &str,
    root_identity: &ResolvedRootIdentity,
) -> PreparedKeyRemapShapeFact {
    let opaque_remap = || {
        PreparedKeyRemapShapeFact::Opaque(decl_slot(
            root_identity,
            LocatorSymbolSpace::Type,
            vec![TypeBodyPathStep::MappedNameType],
        ))
    };
    match name_type {
        // `` `prefix${K & string}` `` or `` `${K & string}suffix` ``
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => {
            // Check for single expression that is K & string or just K
            if expressions.len() == 1 && quasis.len() == 2 {
                let expr_is_param = is_param_or_param_intersect_string(&expressions[0], param);
                if expr_is_param {
                    let prefix = &quasis[0];
                    let suffix = &quasis[1];
                    if suffix.is_empty() && !prefix.is_empty() {
                        return PreparedKeyRemapShapeFact::Prefix(prefix.clone());
                    }
                    if prefix.is_empty() && !suffix.is_empty() {
                        return PreparedKeyRemapShapeFact::Suffix(suffix.clone());
                    }
                }
            }
            opaque_remap()
        }
        // `Capitalize<K & string>` etc.
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.len() == 1 => {
            let arg_is_param = is_param_or_param_intersect_string(&type_arguments[0], param);
            if arg_is_param {
                match &**name {
                    "Capitalize" => {
                        return PreparedKeyRemapShapeFact::CaseTransform(
                            PreparedCaseTransformKind::Capitalize,
                        )
                    }
                    "Uncapitalize" => {
                        return PreparedKeyRemapShapeFact::CaseTransform(
                            PreparedCaseTransformKind::Uncapitalize,
                        )
                    }
                    "Uppercase" => {
                        return PreparedKeyRemapShapeFact::CaseTransform(
                            PreparedCaseTransformKind::Uppercase,
                        )
                    }
                    "Lowercase" => {
                        return PreparedKeyRemapShapeFact::CaseTransform(
                            PreparedCaseTransformKind::Lowercase,
                        )
                    }
                    _ => {}
                }
            }
            opaque_remap()
        }
        _ => opaque_remap(),
    }
}

/// Check if an expression is `K` or `K & string` (common in mapped type name remaps).
fn is_param_or_param_intersect_string(expr: &TypeExpr, param: &str) -> bool {
    // Direct param ref
    if is_bare_ref(expr, param) {
        return true;
    }
    // K & string
    if let TypeExpr::Intersection(parts) = expr {
        if parts.len() == 2 {
            let has_param = parts.iter().any(|p| is_bare_ref(p, param));
            let has_string = parts
                .iter()
                .any(|p| matches!(p, TypeExpr::Primitive(PrimitiveName::String)));
            return has_param && has_string;
        }
    }
    false
}

/// Classify the projection class for a prepared type declaration.
///
/// Priority order:
/// 1. If `member_index` is non-empty and the body only contains direct object
///    members → `DirectMembers` (interfaces, object aliases).
/// 2. If `wrapper_shape.kind` is a recognized structural wrapper → `Wrapper`.
/// 3. If the body is a single `Ref` (possibly parenthesized) → `ForwardSubject`.
/// 4. Otherwise → `Opaque`.
fn classify_projection_inner(
    body: &TypeExpr,
    type_params: &[NarrowTypeParam],
    member_index: &FxHashMap<String, PreparedMemberFact>,
    wrapper_shape: &PreparedWrapperShapeFact,
    root_identity: &ResolvedRootIdentity,
) -> PreparedProjectionClassFact {
    // 1. Direct members — interfaces and object-bodied aliases.
    if !member_index.is_empty() && body_supports_direct_member_projection(body) {
        return PreparedProjectionClassFact::DirectMembers;
    }

    // 2. Structural wrapper — mapped types with recognized patterns.
    if !matches!(wrapper_shape.kind, PreparedWrapperKindFact::None) {
        return PreparedProjectionClassFact::Wrapper;
    }

    // 3. Forward subject — body is a single Ref to another type.
    if let Some(payload) = extract_forward_payload(body, type_params, root_identity) {
        return PreparedProjectionClassFact::ForwardSubject(payload);
    }

    PreparedProjectionClassFact::Opaque
}

fn body_supports_direct_member_projection(body: &TypeExpr) -> bool {
    match body {
        TypeExpr::Object(_) => true,
        TypeExpr::Intersection(parts) => parts.iter().all(body_supports_direct_member_projection),
        TypeExpr::Parenthesized(inner) => body_supports_direct_member_projection(inner),
        _ => false,
    }
}

/// Try to extract a forward-subject payload from a declaration body.
///
/// Matches bodies of the form `Ref { name, type_arguments }` (allowing
/// `Parenthesized` wrapping — parenthesization is transparent to the arg
/// locators). Returns `None` for unions, intersections, conditionals, mapped
/// types, objects, and other non-forwarding shapes.
///
/// Each forwarded argument becomes a content-free [`TypeArgLocator`] of the
/// authored argument position: the empty path addresses the decl body's own
/// arg-bearing `Ref`, and `arg_index` selects the argument in source order.
fn extract_forward_payload(
    body: &TypeExpr,
    type_params: &[NarrowTypeParam],
    root_identity: &ResolvedRootIdentity,
) -> Option<PreparedForwardPayloadFact> {
    match body {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            let forwarding_kind = classify_forwarding_kind(type_arguments, type_params);
            Some(PreparedForwardPayloadFact {
                target_name: name.to_string(),
                forwarding_kind,
                target_args: (0..type_arguments.len())
                    .map(|arg_index| {
                        Some(TypeArgLocator {
                            anchor: decl_anchor(root_identity, LocatorSymbolSpace::Type),
                            path: Vec::new().into(),
                            arg_index: u32::try_from(arg_index).ok()?,
                        })
                    })
                    .collect::<Option<Vec<_>>>()?
                    .into(),
            })
        }
        TypeExpr::Parenthesized(inner) => {
            extract_forward_payload(inner, type_params, root_identity)
        }
        _ => None,
    }
}

/// Determine whether the forwarded args are an identity pass-through of the
/// alias's own type parameters, or an applied (concrete/remapped) alias.
fn classify_forwarding_kind(
    target_args: &[TypeExpr],
    alias_params: &[NarrowTypeParam],
) -> PreparedForwardingKind {
    // Identity: args must be exactly the alias params in order, with no extras.
    if !alias_params.is_empty()
        && target_args.len() == alias_params.len()
        && target_args
            .iter()
            .zip(alias_params.iter())
            .all(|(arg, param)| is_bare_ref(arg, &param.name))
    {
        PreparedForwardingKind::IdentityParams
    } else {
        PreparedForwardingKind::AppliedAlias
    }
}

impl PreparedValueDecl {
    /// Create a new prepared value declaration with minimal fields.
    pub fn new(root_identity: ResolvedRootIdentity, kind: ValueDeclKind) -> Self {
        Self {
            root_identity,
            exported_name: None,
            kind,
            type_annotation: ValueTypeAnnotationFact {
                typeof_alias_target: None,
                classification: ValueAnnotationClass::Absent,
                annotation: None,
            },
            signatures: Vec::new(),
            object_shape: None,
            member_index: FxHashMap::default(),
            enum_members: None,
            external_deps: Vec::new(),
            name_resolution: empty_name_resolution(),
            cache_deps: PreparedCacheDeps::default(),
        }
    }

    /// Classify using the broader PreparedValueDeclKind.
    pub fn prepared_kind(&self) -> PreparedValueDeclKind {
        PreparedValueDeclKind::from(self.kind)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use verter_type_expr::facts::{EnumMemberEntry, EnumScalar};
    use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName};

    use super::*;

    /// The expected member-value locator path for the member at `ordinal`.
    fn member_value_path(ordinal: u32) -> [TypeBodyPathStep; 2] {
        [
            TypeBodyPathStep::Member { ordinal },
            TypeBodyPathStep::MemberValue,
        ]
    }

    #[test]
    fn prepared_type_decl_member_index_from_object_body() {
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "label".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "count".into(),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    true,
                    false,
                )),
            ],
        }));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Props"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        // Each indexed member's `ty` is the content-free LOCATOR of its
        // authored value position — anchored at the declaration, with the
        // source-order member ordinal. No body is stored.
        let label = decl.member("label").expect("label should exist");
        assert!(!label.optional);
        assert_eq!(&*label.ty.anchor.canonical_id, "/types.ts");
        assert_eq!(&*label.ty.anchor.symbol, "Props");
        assert_eq!(label.ty.anchor.space, LocatorSymbolSpace::Type);
        assert_eq!(&*label.ty.path, &member_value_path(0));

        let count = decl.member("count").expect("count should exist");
        assert!(count.optional);
        assert_eq!(&*count.ty.path, &member_value_path(1));

        assert!(decl.member("missing").is_none());
    }

    #[test]
    fn prepared_type_decl_member_index_indexes_method_syntax() {
        // Method-syntax members (`default(props): any`) must be indexed
        // distinctly from property-valued functions (`fn: () => void`),
        // carrying `is_method == true`, so the macro-surface own-member
        // overlay can stamp `declared_in_macro_type_arg` for an own
        // interface method. A property and a method coexist in one body to
        // prove BOTH branches of `index_object_members` run.
        //
        // Discrimination: removing the `ObjectMember::Method` arm from
        // `index_object_members` makes `decl.member("greet")` return `None`
        // (the method is never indexed), so both the `is_some` and the
        // `is_method` assertions below FAIL.
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "label".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                )),
                ObjectMember::Method(verter_type_expr::MethodSignature::synthetic_public(
                    "greet".into(),
                    verter_type_expr::FunctionExpr::synthetic(
                        vec![],
                        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                        vec![],
                    ),
                    false,
                )),
            ],
        }));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Slots"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        // The property member is indexed as a non-method.
        let label = decl.member("label").expect("property `label` indexed");
        assert!(
            !label.is_method,
            "a property member must carry is_method=false",
        );

        // The method-syntax member is indexed AND flagged is_method=true.
        let greet = decl
            .member("greet")
            .expect("method-syntax member `greet` MUST be indexed (the Method branch)");
        assert!(
            greet.is_method,
            "a method-syntax member (`greet(): any`) MUST carry is_method=true; \
             a property-valued function would carry is_method=false",
        );
        // The method's value locator addresses its source-order member
        // position; the dispatch lowers the function shape on demand.
        assert_eq!(&*greet.ty.path, &member_value_path(1));
    }

    #[test]
    fn prepared_member_index_mints_span_origins_and_declaration_origin() {
        // The member-index producer (`index_object_members`) must mint each
        // member's SPAN-RECOVERY ORIGIN (the member ordinal descended from the
        // owning declaration's authored contributor anchor) and stamp the
        // declaration's defining file (`root_identity.canonical_id`) as a
        // typed `DeclarationOrigin` fact. The macro-surface overlay recovers
        // the real spans from the retained parse snapshot via the origin.
        //
        // Discrimination: a producer that drops the contributor anchor, the
        // member ordinal, or the defining-file stamp FAILS the `Authored`
        // equality / `Declared` equality below; a producer that fabricates
        // `Authored` origins for an anchor-less body FAILS the `Synthetic`
        // equality at the end.
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "label".into(),
                    TypeExpr::Primitive(PrimitiveName::String),
                    false,
                    false,
                )),
                ObjectMember::Method(verter_type_expr::MethodSignature::synthetic_public(
                    "greet".into(),
                    verter_type_expr::FunctionExpr::synthetic(
                        vec![],
                        Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                        vec![],
                    ),
                    false,
                )),
            ],
        }));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/decl_origin.ts", "Slots"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(
            &body,
            Some(DeclContributorAnchor {
                contributor_index: 3,
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 3,
            }),
        );

        // PROPERTY member: authored span origin (ordinal 0 under the
        // contributor anchor) + the declaration's defining file.
        let label = decl.member("label").expect("property `label` indexed");
        assert_eq!(
            label.span_origin,
            MemberSpansOrigin::Authored {
                anchor: DeclContributorAnchor {
                    contributor_index: 3,
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    owner_local_ordinal: 3,
                },
                member_path: Arc::from(vec![0u32]),
            },
            "property span origin must descend [0] from the contributor anchor"
        );
        assert_eq!(
            label.declaration_origin,
            DeclarationOrigin::Declared(Arc::from("/decl_origin.ts")),
            "member fact must stamp the declaration's defining file"
        );

        // METHOD member: authored span origin (ordinal 1) + defining file.
        let greet = decl.member("greet").expect("method `greet` indexed");
        assert_eq!(
            greet.span_origin,
            MemberSpansOrigin::Authored {
                anchor: DeclContributorAnchor {
                    contributor_index: 3,
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    owner_local_ordinal: 3,
                },
                member_path: Arc::from(vec![1u32]),
            },
        );
        assert_eq!(
            greet.declaration_origin,
            DeclarationOrigin::Declared(Arc::from("/decl_origin.ts")),
        );

        // NEGATIVE: a genuinely-absent member is still absent (the producer
        // did not fabricate entries).
        assert!(decl.member("missing").is_none());

        // A GENUINELY SYNTHETIC body (no contributor anchor) records explicit
        // Synthetic origins — never a fabricated authored position.
        let mut synthetic = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/decl_origin.ts", "SynthSlots"),
            TypeDeclKind::Interface,
        );
        synthetic.build_member_index(&body, None);
        assert_eq!(
            synthetic.member("label").unwrap().span_origin,
            MemberSpansOrigin::Synthetic(SourceSynthetic),
        );
    }

    #[test]
    fn prepared_decl_kind_from_type_decl_kind() {
        assert_eq!(
            PreparedDeclKind::from(TypeDeclKind::Alias),
            PreparedDeclKind::Alias
        );
        assert_eq!(
            PreparedDeclKind::from(TypeDeclKind::Interface),
            PreparedDeclKind::Interface
        );
        assert_eq!(
            PreparedDeclKind::from(TypeDeclKind::Class),
            PreparedDeclKind::Class
        );
    }

    #[test]
    fn prepared_cache_deps_valid_when_hashes_match() {
        let deps = PreparedCacheDeps {
            defining_file: Some(("/types.ts".into(), 12345)),
            barrel_participants: vec![("/barrel.ts".into(), 67890)],
            local_closure_participants: vec!["Inner".into()],
        };

        let mut hashes = FxHashMap::default();
        hashes.insert("/types.ts".to_string(), 12345u64);
        hashes.insert("/barrel.ts".to_string(), 67890u64);
        assert!(deps.is_valid(&hashes));

        // Stale defining file
        hashes.insert("/types.ts".to_string(), 99999u64);
        assert!(!deps.is_valid(&hashes));
    }

    #[test]
    fn prepared_cache_deps_invalid_when_barrel_changes() {
        let deps = PreparedCacheDeps {
            defining_file: Some(("/types.ts".into(), 100)),
            barrel_participants: vec![("/index.ts".into(), 200)],
            local_closure_participants: vec![],
        };

        let mut hashes = FxHashMap::default();
        hashes.insert("/types.ts".to_string(), 100u64);
        hashes.insert("/index.ts".to_string(), 300u64); // changed
        assert!(!deps.is_valid(&hashes));
    }

    #[test]
    fn prepared_value_decl_enum_members() {
        let mut decl = PreparedValueDecl::new(
            ResolvedRootIdentity::new("/enums.ts", "Color"),
            ValueDeclKind::Const,
        );

        decl.enum_members = Some(EnumMemberFact {
            members: Arc::from(vec![
                EnumMemberEntry {
                    name: "Red".to_string(),
                    value: EnumScalar::Number("0".to_string()),
                },
                EnumMemberEntry {
                    name: "Green".to_string(),
                    value: EnumScalar::Number("1".to_string()),
                },
            ]),
        });

        let enum_members = &decl.enum_members.as_ref().unwrap().members;
        assert_eq!(enum_members.len(), 2);
        assert!(enum_members.iter().any(|entry| entry.name == "Red"));
        // Source order is preserved (TS enum members are ordered), and each
        // member carries its closed scalar value.
        assert_eq!(enum_members[0].name, "Red");
        assert_eq!(enum_members[0].value, EnumScalar::Number("0".to_string()));
        assert_eq!(enum_members[1].name, "Green");
        assert_eq!(enum_members[1].value, EnumScalar::Number("1".to_string()));
    }

    #[test]
    fn prepared_type_decl_prepared_kind() {
        let decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "T"),
            TypeDeclKind::Interface,
        );
        assert_eq!(decl.prepared_kind(), PreparedDeclKind::Interface);
        // The minted body facts classify from the kind and address the whole
        // authored body (empty path at the declaration's own anchor).
        assert_eq!(decl.body_facts.classification, TypeBodyClass::Interface);
        assert_eq!(&*decl.body_facts.body_slot.anchor.canonical_id, "/t.ts");
        assert_eq!(&*decl.body_facts.body_slot.anchor.symbol, "T");
        assert!(decl.body_facts.body_slot.path.is_empty());
        assert!(decl.body_facts.merged_contributor_slots.is_empty());
    }

    #[test]
    fn set_merged_contributors_mints_ordered_contributor_slots() {
        // A same-name merged interface records one contributor slot per
        // ordinal and flips the classification to MergedInterface; a zero
        // count resets to the non-merged state.
        //
        // Discrimination: a producer that stops minting per-ordinal slots (or
        // forgets the classification flip) fails the slot-path / class
        // equalities below.
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Merged"),
            TypeDeclKind::Interface,
        );
        decl.set_merged_contributors(2).unwrap();

        assert_eq!(
            decl.body_facts.classification,
            TypeBodyClass::MergedInterface
        );
        assert_eq!(decl.body_facts.merged_contributor_slots.len(), 2);
        for (ordinal, slot) in decl.body_facts.merged_contributor_slots.iter().enumerate() {
            assert_eq!(&*slot.anchor.symbol, "Merged");
            assert_eq!(
                &*slot.path,
                &[TypeBodyPathStep::MergedContributor {
                    ordinal: ordinal as u32
                }],
            );
        }

        decl.set_merged_contributors(0).unwrap();
        assert!(decl.body_facts.merged_contributor_slots.is_empty());
        assert_eq!(decl.body_facts.classification, TypeBodyClass::Interface);
    }

    #[test]
    fn merged_contributor_count_overflow_is_typed_and_non_mutating() {
        if usize::BITS <= u32::BITS {
            return;
        }
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Merged"),
            TypeDeclKind::Interface,
        );
        let original = decl.body_facts.classification;
        let count = u32::MAX as usize + 1;
        assert_eq!(
            decl.set_merged_contributors(count),
            Err(AuthoredOrdinalOverflow { count })
        );
        assert_eq!(decl.body_facts.classification, original);
        assert!(decl.body_facts.merged_contributor_slots.is_empty());
    }

    // -----------------------------------------------------------------------
    // Intersection member indexing tests
    // -----------------------------------------------------------------------

    fn make_object(props: &[(&str, TypeExpr, bool)]) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: props
                .iter()
                .map(|(name, ty, optional)| {
                    ObjectMember::Property(ObjectProperty::synthetic_public(
                        (*name).into(),
                        ty.clone(),
                        *optional,
                        false,
                    ))
                })
                .collect(),
        }))
    }

    #[test]
    fn intersection_tail_object_members_indexed() {
        // Simulates: interface Foo extends Bar { own: string }
        // Lowered as: Intersection([Ref("Bar"), Object({ own: string })])
        let body = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::named("Bar"), // heritage ref
            make_object(&[("own", TypeExpr::Primitive(PrimitiveName::String), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Foo"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        // KEY ASSERTION: 'own' from the intersection tail is indexed with the
        // RAW body path — the intersection-root body needs the arm step (the
        // deref fails a bare `Member` step on an intersection), then the raw
        // member index within that object.
        let own = decl
            .member("own")
            .expect("own member from intersection tail should be indexed");
        assert_eq!(
            &*own.ty.path,
            &[
                TypeBodyPathStep::IntersectionArm { ordinal: 1 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        );

        // Negative: 'Bar' heritage ref members should NOT be indexed
        // (they don't exist as direct members)
        assert!(
            decl.member("Bar").is_none(),
            "heritage ref name should not be indexed as a member"
        );
    }

    #[test]
    fn intersection_own_members_win_over_heritage() {
        // interface Foo extends Bar { mode: number }
        // where Bar also has mode: string
        // Lowered as: Intersection([Object({mode: string}), Object({mode?: number})])
        // — the own (last) slice declares `mode` OPTIONAL so the winning
        // header facts are observable.
        let body = TypeExpr::Intersection(Arc::from(vec![
            make_object(&[("mode", TypeExpr::Primitive(PrimitiveName::String), false)]),
            make_object(&[("mode", TypeExpr::Primitive(PrimitiveName::Number), true)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Foo"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        // Right-to-left precedence: the LAST object in the intersection wins
        // the indexed header facts (here: `optional == true`). Removing the
        // reverse traversal makes the first slice (`optional == false`) win
        // and FAILS this assertion.
        let mode = decl.member("mode").expect("mode should be indexed");
        assert!(
            mode.optional,
            "own member (last in intersection) should win the indexed facts"
        );
        // The winning fact's locator addresses where the winning member
        // actually lives: arm 1 (its RAW source index), member 0 within it —
        // never the shadowed arm-0 position.
        assert_eq!(
            &*mode.ty.path,
            &[
                TypeBodyPathStep::IntersectionArm { ordinal: 1 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        );
    }

    #[test]
    fn non_object_body_still_produces_empty_index() {
        let body = TypeExpr::Primitive(PrimitiveName::String);
        let mut decl =
            PreparedTypeDecl::new(ResolvedRootIdentity::new("/t.ts", "T"), TypeDeclKind::Alias);
        decl.build_member_index(&body, None);

        assert!(
            decl.member_index.is_empty(),
            "primitive body should have empty member index"
        );
    }

    #[test]
    fn interface_with_two_heritage_clauses_and_own_tail() {
        // interface Foo extends A, B { own: boolean }
        // Lowered as: Intersection([Ref("A"), Ref("B"), Object({own: boolean})])
        let body = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::named("A"),
            TypeExpr::named("B"),
            make_object(&[("own", TypeExpr::Primitive(PrimitiveName::Boolean), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Foo"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        // Heritage refs contribute no members, but they still occupy their
        // RAW arm positions: the trailing object is arm 2, and `own` is its
        // member 0.
        let own = decl
            .member("own")
            .expect("own member should be indexed from trailing object");
        assert_eq!(
            &*own.ty.path,
            &[
                TypeBodyPathStep::IntersectionArm { ordinal: 2 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        );
        // Heritage Ref names should NOT appear as members
        assert!(decl.member("A").is_none());
        assert!(decl.member("B").is_none());
    }

    #[test]
    fn missing_member_returns_none_without_panic() {
        let body = make_object(&[(
            "existing",
            TypeExpr::Primitive(PrimitiveName::String),
            false,
        )]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "T"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        assert!(decl.member("existing").is_some());
        assert!(
            decl.member("nonexistent").is_none(),
            "missing member should return None"
        );
        assert!(
            decl.member("").is_none(),
            "empty string member should return None"
        );
    }

    #[test]
    fn generic_alias_body_with_type_args_not_indexed() {
        // type Wrapper<T> = Array<T> — generic ref body, NOT indexable
        let body = TypeExpr::named_with_args("Array", vec![TypeExpr::named("T")]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Wrapper"),
            TypeDeclKind::Alias,
        );
        decl.build_member_index(&body, None);

        assert!(
            decl.member_index.is_empty(),
            "generic ref body should not be indexed"
        );
    }

    #[test]
    fn merged_interface_nested_intersections_keep_earlier_members() {
        let body = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::Intersection(Arc::from(vec![make_object(&[(
                "first",
                TypeExpr::Primitive(PrimitiveName::String),
                false,
            )])])),
            make_object(&[("second", TypeExpr::Primitive(PrimitiveName::Number), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Merged"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        // Both slices are indexed; each locator is the RAW body path — one
        // `IntersectionArm` step per intersection LEVEL (the deref selects one
        // level per step), then the raw member index within its object.
        let first = decl.member("first").expect("first indexed");
        assert_eq!(
            &*first.ty.path,
            &[
                TypeBodyPathStep::IntersectionArm { ordinal: 0 },
                TypeBodyPathStep::IntersectionArm { ordinal: 0 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        );
        let second = decl.member("second").expect("second indexed");
        assert_eq!(
            &*second.ty.path,
            &[
                TypeBodyPathStep::IntersectionArm { ordinal: 1 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        );
    }

    #[test]
    fn nameless_signature_positions_keep_named_member_raw_ordinal() {
        // `{ (x: string): void; p: number }` — the deref selects
        // `obj.properties[ordinal]`, and the nameless call signature occupies
        // raw index 0. A producer that compacted named members to a
        // NAME-ordinal would store `Member { ordinal: 0 }` for `p` and
        // mis-address the call signature.
        let body = TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![
                ObjectMember::CallSignature(verter_type_expr::FunctionExpr::synthetic(
                    vec![],
                    Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Void))),
                    vec![],
                )),
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    "p".into(),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    false,
                    false,
                )),
            ],
        }));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Callable"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(
            &body,
            Some(DeclContributorAnchor {
                contributor_index: 0,
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
            }),
        );

        let p = decl.member("p").expect("named member indexed");
        assert_eq!(&*p.ty.path, &member_value_path(1));
        // The span-recovery origin indexes the same RAW authored member
        // surface — the call signature occupies position 0 there too.
        assert_eq!(
            p.span_origin,
            MemberSpansOrigin::Authored {
                anchor: DeclContributorAnchor {
                    contributor_index: 0,
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    owner_local_ordinal: 0,
                },
                member_path: Arc::from(vec![1u32]),
            },
        );
    }

    #[test]
    fn intersection_root_member_locator_carries_raw_arm_step() {
        // An intersection-root body is NOT navigable by a bare `Member` step
        // (the deref fails closed on the shape mismatch): the stored locator
        // must carry the `IntersectionArm` descent with the arm's RAW source
        // index. A parenthesized wrapper is structurally transparent to the
        // deref and must take NO path step.
        let body = TypeExpr::Intersection(Arc::from(vec![
            TypeExpr::named("Base"),
            TypeExpr::Parenthesized(Arc::new(make_object(&[(
                "wrapped",
                TypeExpr::Primitive(PrimitiveName::String),
                false,
            )]))),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "Wrapped"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);

        let wrapped = decl.member("wrapped").expect("wrapped indexed");
        assert_eq!(
            &*wrapped.ty.path,
            &[
                TypeBodyPathStep::IntersectionArm { ordinal: 1 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        );
    }

    #[test]
    fn intersection_arm_members_record_synthetic_span_origin() {
        // `MemberSpansOrigin::Authored { member_path }` descends TOP-LEVEL
        // decl-body member ordinals only — it has no intersection-arm step, so
        // a member reached through an `IntersectionArm` descent has no
        // representable authored position. The producer must record the honest
        // typed miss (`Synthetic`), never an `Authored` origin claiming a
        // position the schema cannot address.
        //
        // Discrimination: a producer that mints `Authored { member_path:
        // [raw_index] }` for intersection-arm members FAILS the `Synthetic`
        // equalities below.
        //
        // `type T = { a: string } & { b: number }`
        let body = TypeExpr::Intersection(Arc::from(vec![
            make_object(&[("a", TypeExpr::Primitive(PrimitiveName::String), false)]),
            make_object(&[("b", TypeExpr::Primitive(PrimitiveName::Number), false)]),
        ]));

        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "T"),
            TypeDeclKind::Alias,
        );
        decl.build_member_index(
            &body,
            Some(DeclContributorAnchor {
                contributor_index: 0,
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
            }),
        );

        // Both intersection-arm members carry the explicit typed miss.
        assert_eq!(
            decl.member("a").expect("a indexed").span_origin,
            MemberSpansOrigin::Synthetic(SourceSynthetic),
            "an intersection-arm member has no representable authored span \
             origin — it must record Synthetic, not a top-level-ordinal \
             Authored",
        );
        let b = decl.member("b").expect("b indexed");
        assert_eq!(b.span_origin, MemberSpansOrigin::Synthetic(SourceSynthetic));
        // The BODY locator stays intersection-truthful (raw arm + member
        // steps) — the span-origin miss does not degrade the value locator.
        assert_eq!(
            &*b.ty.path,
            &[
                TypeBodyPathStep::IntersectionArm { ordinal: 1 },
                TypeBodyPathStep::Member { ordinal: 0 },
                TypeBodyPathStep::MemberValue,
            ],
        );

        // CONTROL: `type U = { a: string }` — a plain top-level object body
        // under the same anchor keeps the recoverable `Authored` origin with
        // the raw member ordinal.
        let plain = make_object(&[("a", TypeExpr::Primitive(PrimitiveName::String), false)]);
        let mut plain_decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/types.ts", "U"),
            TypeDeclKind::Alias,
        );
        plain_decl.build_member_index(
            &plain,
            Some(DeclContributorAnchor {
                contributor_index: 0,
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                owner_local_ordinal: 0,
            }),
        );
        assert_eq!(
            plain_decl.member("a").expect("a indexed").span_origin,
            MemberSpansOrigin::Authored {
                anchor: DeclContributorAnchor {
                    contributor_index: 0,
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    owner_local_ordinal: 0,
                },
                member_path: Arc::from(vec![0u32]),
            },
            "a top-level object member keeps the recoverable Authored origin",
        );
    }

    // -----------------------------------------------------------------------
    // Wrapper shape classification tests
    // -----------------------------------------------------------------------

    fn make_type_param(name: &str, ordinal: u32) -> NarrowTypeParam {
        NarrowTypeParam {
            name: name.into(),
            ordinal,
            constraint: None,
            default: None,
        }
    }

    /// Helper: `{ [K in keyof T]: T[K] }` — identity mapped type
    fn identity_mapped_body(base: &str, param: &str) -> TypeExpr {
        TypeExpr::Mapped {
            parameter: param.into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named(base)))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named(base)),
                index: Arc::new(TypeExpr::named(param)),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: None,
        }
    }

    #[test]
    fn classify_identity_mapped_type() {
        let body = identity_mapped_body("T", "K");
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Identity"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::Identity);
        assert_eq!(decl.wrapper_shape.source_param_index, Some(0));
        assert!(matches!(
            decl.wrapper_shape.value_rule,
            PreparedValueRuleShapeFact::PassThrough
        ));
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShapeFact::Identity
        ));
        // Negative: must not be PureOverlay
        assert_ne!(
            decl.wrapper_shape.kind,
            PreparedWrapperKindFact::PureOverlay
        );
    }

    #[test]
    fn classify_partial_overlay() {
        // { [K in keyof T]?: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::Add,
            readonly: MappedModifier::None,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "MyPartial"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(
            decl.wrapper_shape.kind,
            PreparedWrapperKindFact::PureOverlay
        );
        assert_eq!(decl.wrapper_shape.modifiers.optional, Some(true));
        // Negative: readonly unchanged, key remap is identity, value is passthrough
        assert_eq!(decl.wrapper_shape.modifiers.readonly, None);
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShapeFact::Identity
        ));
        assert!(matches!(
            decl.wrapper_shape.value_rule,
            PreparedValueRuleShapeFact::PassThrough
        ));
    }

    #[test]
    fn classify_required_overlay() {
        // { [K in keyof T]-?: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::Remove,
            readonly: MappedModifier::None,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "MyRequired"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(
            decl.wrapper_shape.kind,
            PreparedWrapperKindFact::PureOverlay
        );
        assert_eq!(decl.wrapper_shape.modifiers.optional, Some(false));
        // Negative: readonly unchanged
        assert_eq!(decl.wrapper_shape.modifiers.readonly, None);
    }

    #[test]
    fn classify_readonly_overlay() {
        // { readonly [K in keyof T]: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::Add,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "MyReadonly"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(
            decl.wrapper_shape.kind,
            PreparedWrapperKindFact::PureOverlay
        );
        assert_eq!(decl.wrapper_shape.modifiers.readonly, Some(true));
        // Negative: optional unchanged
        assert_eq!(decl.wrapper_shape.modifiers.optional, None);
    }

    #[test]
    fn classify_prefix_key_remap() {
        // { [K in keyof T as `data-${K & string}`]: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::TemplateLiteral {
                quasis: vec!["data-".into(), String::new()],
                expressions: Arc::from(vec![TypeExpr::Intersection(Arc::from(vec![
                    TypeExpr::named("K"),
                    TypeExpr::Primitive(PrimitiveName::String),
                ]))]),
            })),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "DataPrefixed"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::KeyRemap);
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShapeFact::Prefix(ref p) if p == "data-"
        ));
        // Negative: value is still passthrough
        assert!(matches!(
            decl.wrapper_shape.value_rule,
            PreparedValueRuleShapeFact::PassThrough
        ));
    }

    #[test]
    fn classify_capitalize_key_remap() {
        // { [K in keyof T as Capitalize<K & string>]: T[K] }
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::named_with_args(
                "Capitalize",
                vec![TypeExpr::Intersection(Arc::from(vec![
                    TypeExpr::named("K"),
                    TypeExpr::Primitive(PrimitiveName::String),
                ]))],
            ))),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "CapKeys"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::KeyRemap);
        assert!(matches!(
            decl.wrapper_shape.key_remap,
            PreparedKeyRemapShapeFact::CaseTransform(PreparedCaseTransformKind::Capitalize)
        ));
    }

    #[test]
    fn classify_opaque_key_remap_mints_name_type_locator() {
        // { [K in keyof T as Weird<K, K>]: T[K] } — an unrecognized remap is
        // the content-free LOCATOR of the authored `as`-clause position
        // ([MappedNameType] at the declaration's own anchor), never an
        // embedded body.
        //
        // Discrimination: the pre-narrowing shape stored the remap expression
        // itself; a producer that stops minting the authored position (or
        // anchors it elsewhere) fails the locator equality below.
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::IndexedAccess {
                object: Arc::new(TypeExpr::named("T")),
                index: Arc::new(TypeExpr::named("K")),
            }),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: Some(Arc::new(TypeExpr::named_with_args(
                "Weird",
                vec![TypeExpr::named("K"), TypeExpr::named("K")],
            ))),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "WeirdKeys"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::KeyRemap);
        match &decl.wrapper_shape.key_remap {
            PreparedKeyRemapShapeFact::Opaque(slot) => {
                assert_eq!(&*slot.anchor.canonical_id, "/t.ts");
                assert_eq!(&*slot.anchor.symbol, "WeirdKeys");
                assert_eq!(&*slot.path, &[TypeBodyPathStep::MappedNameType]);
            }
            other => panic!("expected Opaque(name-type locator), got {:?}", other),
        }
    }

    #[test]
    fn classify_identity_alias_not_wrapper() {
        // type Alias<T, U> = Other<T, U>
        // This is an identity-forwarding alias, NOT a structural wrapper.
        // Handled by PreparedProjectionClassFact::ForwardSubject(IdentityParams).
        let body =
            TypeExpr::named_with_args("Other", vec![TypeExpr::named("T"), TypeExpr::named("U")]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Alias"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0), make_type_param("U", 1)];
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        // Wrapper shape: None — not a mapped type
        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::None);
        assert_eq!(decl.wrapper_shape.source_param_index, None);
        // Projection class: ForwardSubject(IdentityParams)
        match &decl.projection_class {
            PreparedProjectionClassFact::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "Other");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::IdentityParams
                );
            }
            other => panic!("expected ForwardSubject(IdentityParams), got {:?}", other),
        }
    }

    #[test]
    fn classify_non_structural_body() {
        // type Foo<T> = T extends string ? T : never
        let body = TypeExpr::Conditional {
            check: Arc::new(TypeExpr::named("T")),
            extends: Arc::new(TypeExpr::Primitive(PrimitiveName::String)),
            true_type: Arc::new(TypeExpr::named("T")),
            false_type: Arc::new(TypeExpr::Primitive(PrimitiveName::Never)),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Foo"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::None);
        // Negative: source_param_index is None for non-structural bodies
        assert_eq!(decl.wrapper_shape.source_param_index, None);
    }

    #[test]
    fn classify_value_transform_not_identity() {
        // { [K in keyof T]: Wrap<T[K]> } — value transform, not identity
        let body = TypeExpr::Mapped {
            parameter: "K".into(),
            source: Arc::new(TypeExpr::KeyOf(Arc::new(TypeExpr::named("T")))),
            value: Arc::new(TypeExpr::named_with_args(
                "Wrap",
                vec![TypeExpr::IndexedAccess {
                    object: Arc::new(TypeExpr::named("T")),
                    index: Arc::new(TypeExpr::named("K")),
                }],
            )),
            optional: MappedModifier::None,
            readonly: MappedModifier::None,
            name_type: None,
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Wrapped"),
            TypeDeclKind::Alias,
        );
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.classify_wrapper_shape(&body);

        // Has a value transform, so not Identity or PureOverlay. The
        // transform is the LOCATOR of the authored mapped-value position
        // ([MappedValue] at the declaration's own anchor) — never an
        // embedded body.
        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::None);
        match &decl.wrapper_shape.value_rule {
            PreparedValueRuleShapeFact::Transform(slot) => {
                assert_eq!(&*slot.anchor.canonical_id, "/t.ts");
                assert_eq!(&*slot.anchor.symbol, "Wrapped");
                assert_eq!(&*slot.path, &[TypeBodyPathStep::MappedValue]);
            }
            other => panic!("expected Transform(mapped-value locator), got {:?}", other),
        }
    }

    #[test]
    fn classify_no_type_params_is_none() {
        // type Foo = string — no type params, no wrapper
        let body = TypeExpr::Primitive(PrimitiveName::String);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Foo"),
            TypeDeclKind::Alias,
        );
        decl.classify_wrapper_shape(&body);

        assert_eq!(decl.wrapper_shape.kind, PreparedWrapperKindFact::None);
        // Negative: no source param for non-generic types
        assert_eq!(decl.wrapper_shape.source_param_index, None);
    }

    // -----------------------------------------------------------------------
    // Projection classification tests
    // -----------------------------------------------------------------------

    #[test]
    fn projection_interface_is_direct_members() {
        // interface Props { msg: string }
        let body = make_object(&[("msg", TypeExpr::Primitive(PrimitiveName::String), false)]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Props"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClassFact::DirectMembers
        ));
    }

    #[test]
    fn projection_object_alias_is_direct_members() {
        // type Props = { msg: string }
        let body = make_object(&[("msg", TypeExpr::Primitive(PrimitiveName::String), false)]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Props"),
            TypeDeclKind::Alias,
        );
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClassFact::DirectMembers
        ));
    }

    #[test]
    fn projection_identity_alias_is_forward_identity() {
        // type A<T> = B<T>
        let body = TypeExpr::Ref {
            name: "B".into(),
            type_arguments: vec![TypeExpr::named("T")].into(),
        };
        let mut decl =
            PreparedTypeDecl::new(ResolvedRootIdentity::new("/t.ts", "A"), TypeDeclKind::Alias);
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        match &decl.projection_class {
            PreparedProjectionClassFact::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "B");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::IdentityParams
                );
            }
            other => panic!("expected ForwardSubject(IdentityParams), got {:?}", other),
        }
    }

    #[test]
    fn projection_concrete_alias_is_forward_applied() {
        // type ChatShimmer = ComponentConfig<Theme, AppConfig, 'chatShimmer'>
        let body = TypeExpr::Ref {
            name: "ComponentConfig".into(),
            type_arguments: vec![
                TypeExpr::named("Theme"),
                TypeExpr::named("AppConfig"),
                TypeExpr::string_literal("chatShimmer"),
            ]
            .into(),
        };
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "ChatShimmer"),
            TypeDeclKind::Alias,
        );
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        match &decl.projection_class {
            PreparedProjectionClassFact::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "ComponentConfig");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::AppliedAlias
                );
                // The forwarded args are content-free LOCATORS of the authored
                // argument positions: one per source-order arg_index, anchored
                // at the declaration's own body (empty path).
                assert_eq!(payload.target_args.len(), 3);
                for (index, arg) in payload.target_args.iter().enumerate() {
                    assert_eq!(arg.arg_index, index as u32);
                    assert_eq!(&*arg.anchor.canonical_id, "/t.ts");
                    assert_eq!(&*arg.anchor.symbol, "ChatShimmer");
                    assert!(arg.path.is_empty());
                }
            }
            other => panic!("expected ForwardSubject(AppliedAlias), got {:?}", other),
        }
    }

    #[test]
    fn projection_partial_remap_alias_is_forward_applied() {
        // type A<T> = B<T, string>
        let body = TypeExpr::Ref {
            name: "B".into(),
            type_arguments: vec![
                TypeExpr::named("T"),
                TypeExpr::Primitive(PrimitiveName::String),
            ]
            .into(),
        };
        let mut decl =
            PreparedTypeDecl::new(ResolvedRootIdentity::new("/t.ts", "A"), TypeDeclKind::Alias);
        decl.type_parameters = vec![make_type_param("T", 0)];
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        match &decl.projection_class {
            PreparedProjectionClassFact::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "B");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::AppliedAlias
                );
            }
            other => panic!("expected ForwardSubject(AppliedAlias), got {:?}", other),
        }
    }

    #[test]
    fn projection_union_is_opaque() {
        // type A = string | number — not a forward subject
        let body = TypeExpr::Union(
            vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]
            .into(),
        );
        let mut decl =
            PreparedTypeDecl::new(ResolvedRootIdentity::new("/t.ts", "A"), TypeDeclKind::Alias);
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClassFact::Opaque
        ));
    }

    #[test]
    fn projection_intersection_is_opaque() {
        // type A = B & C — not a forward subject
        let body = TypeExpr::Intersection(vec![TypeExpr::named("B"), TypeExpr::named("C")].into());
        let mut decl =
            PreparedTypeDecl::new(ResolvedRootIdentity::new("/t.ts", "A"), TypeDeclKind::Alias);
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClassFact::Opaque
        ));
    }

    #[test]
    fn projection_interface_with_heritage_and_own_members_is_opaque() {
        // interface Props extends Base { own: string }
        let body = TypeExpr::intersection(vec![
            TypeExpr::named("Base"),
            make_object(&[("own", TypeExpr::Primitive(PrimitiveName::String), false)]),
        ]);
        let mut decl = PreparedTypeDecl::new(
            ResolvedRootIdentity::new("/t.ts", "Props"),
            TypeDeclKind::Interface,
        );
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        assert!(matches!(
            decl.projection_class,
            PreparedProjectionClassFact::Opaque
        ));
    }

    #[test]
    fn projection_parenthesized_ref_is_forward() {
        // type A = (B<X>) — parenthesized refs still classify as forwarded
        let body = TypeExpr::Parenthesized(Arc::new(TypeExpr::Ref {
            name: "B".into(),
            type_arguments: vec![TypeExpr::named("X")].into(),
        }));
        let mut decl =
            PreparedTypeDecl::new(ResolvedRootIdentity::new("/t.ts", "A"), TypeDeclKind::Alias);
        decl.build_member_index(&body, None);
        decl.classify_wrapper_shape(&body);
        decl.classify_projection(&body);

        match &decl.projection_class {
            PreparedProjectionClassFact::ForwardSubject(payload) => {
                assert_eq!(payload.target_name, "B");
                assert_eq!(
                    payload.forwarding_kind,
                    PreparedForwardingKind::AppliedAlias
                );
                // Parenthesization is transparent: the arg locator still
                // addresses the body's own arg-bearing position.
                assert_eq!(payload.target_args.len(), 1);
                assert_eq!(payload.target_args[0].arg_index, 0);
                assert!(payload.target_args[0].path.is_empty());
            }
            other => panic!("expected ForwardSubject, got {:?}", other),
        }
    }
}

#[cfg(test)]
mod key_domain_closedness_producer_tests {
    //! Discriminating fixtures for the KEY-DOMAIN closedness fact producer:
    //! each asserts a specific recipe arm / path / shape verdict a perturbed
    //! producer could not reproduce (no always-true predicates).

    use std::sync::Arc;

    use verter_type_expr::facts::ClosednessRecipe;
    use verter_type_expr::{
        empty_type_args, IndexSignature, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
        TypeParam,
    };

    use super::*;

    fn ident() -> ResolvedRootIdentity {
        ResolvedRootIdentity::new("/types.ts", "Props")
    }

    fn object_body() -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "label".into(),
                TypeExpr::Primitive(PrimitiveName::String),
                false,
                false,
            ))],
        }))
    }

    fn object_with_index_key(key: TypeExpr) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::IndexSignature(IndexSignature::synthetic(
                "k".into(),
                key,
                TypeExpr::Primitive(PrimitiveName::String),
                false,
            ))],
        }))
    }

    fn bare_ref(name: &str) -> TypeExpr {
        TypeExpr::Ref {
            name: Arc::from(name),
            type_arguments: empty_type_args(),
        }
    }

    #[test]
    fn plain_object_body_mints_object_closed_and_shape() {
        let fact = collect_key_domain_closedness_fact(&ident(), &[object_body()], false);
        assert!(fact.closed_object_shape);
        assert_eq!(&*fact.body_recipes, &[ClosednessRecipe::ObjectClosed]);
    }

    #[test]
    fn scalar_index_key_stays_object_closed_but_param_key_escapes() {
        let scalar = collect_key_domain_closedness_fact(
            &ident(),
            &[object_with_index_key(TypeExpr::Primitive(
                PrimitiveName::String,
            ))],
            false,
        );
        assert_eq!(&*scalar.body_recipes, &[ClosednessRecipe::ObjectClosed]);

        let param_key = collect_key_domain_closedness_fact(
            &ident(),
            &[object_with_index_key(TypeExpr::TypeParameter(TypeParam {
                name: "K".into(),
                constraint: None,
                default: None,
            }))],
            false,
        );
        match &param_key.body_recipes[..] {
            [ClosednessRecipe::LowerAndClassify { slot }] => {
                assert_eq!(&*slot.anchor.canonical_id, "/types.ts");
                assert_eq!(&*slot.anchor.symbol, "Props");
                assert!(slot.path.is_empty(), "whole-body escape has empty path");
            }
            other => panic!("expected whole-object escape, got {other:?}"),
        }
        // The closed-object SHAPE verdict is the nominal carve-out (pure
        // member-set syntax) — it holds for BOTH objects.
        assert!(scalar.closed_object_shape);
        assert!(param_key.closed_object_shape);
    }

    #[test]
    fn union_of_literals_mints_all_arms_and_is_not_object_shape() {
        let body = TypeExpr::Union(Arc::from(
            vec![
                TypeExpr::Literal(verter_type_expr::LiteralValue::String("a".into())),
                TypeExpr::Primitive(PrimitiveName::Number),
            ]
            .into_boxed_slice(),
        ));
        let fact = collect_key_domain_closedness_fact(&ident(), &[body], false);
        assert!(!fact.closed_object_shape, "a union is not an object shape");
        assert_eq!(
            &*fact.body_recipes,
            &[ClosednessRecipe::AllArms(Arc::from(
                vec![ClosednessRecipe::ClosedLeaf, ClosednessRecipe::ClosedLeaf].into_boxed_slice()
            ))]
        );
    }

    #[test]
    fn intersection_escape_carries_the_arm_path() {
        let generic_arm = TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: Arc::from(vec![bare_ref("T")].into_boxed_slice()),
        };
        let body = TypeExpr::Intersection(Arc::from(
            vec![object_body(), generic_arm].into_boxed_slice(),
        ));
        let fact = collect_key_domain_closedness_fact(&ident(), &[body], false);
        let [ClosednessRecipe::AllArms(arms)] = &fact.body_recipes[..] else {
            panic!("expected one AllArms recipe, got {:?}", fact.body_recipes);
        };
        assert_eq!(arms[0], ClosednessRecipe::ObjectClosed);
        match &arms[1] {
            ClosednessRecipe::LowerAndClassify { slot } => {
                assert_eq!(
                    &*slot.path,
                    &[TypeBodyPathStep::IntersectionArm { ordinal: 1 }]
                );
            }
            other => panic!("expected arm escape, got {other:?}"),
        }
    }

    #[test]
    fn merged_bodies_mint_per_contributor_with_merged_prefix() {
        let generic = TypeExpr::Ref {
            name: Arc::from("Foo"),
            type_arguments: Arc::from(vec![bare_ref("T")].into_boxed_slice()),
        };
        let fact =
            collect_key_domain_closedness_fact(&ident(), &[object_body(), generic.clone()], true);
        assert_eq!(fact.body_recipes.len(), 2);
        assert_eq!(fact.body_recipes[0], ClosednessRecipe::ObjectClosed);
        match &fact.body_recipes[1] {
            ClosednessRecipe::LowerAndClassify { slot } => {
                assert_eq!(
                    &*slot.path,
                    &[TypeBodyPathStep::MergedContributor { ordinal: 1 }]
                );
            }
            other => panic!("expected merged-contributor escape, got {other:?}"),
        }
        assert!(
            !fact.closed_object_shape,
            "a non-object contributor breaks the shape fold"
        );

        // A SINGLE group mints from the primary (last-wins) body only.
        let single = collect_key_domain_closedness_fact(&ident(), &[generic, object_body()], false);
        assert_eq!(&*single.body_recipes, &[ClosednessRecipe::ObjectClosed]);
        assert!(single.closed_object_shape);
    }

    #[test]
    fn leaf_arms_discriminate_by_shape() {
        let cases = [
            (
                bare_ref("Alias"),
                ClosednessRecipe::FollowRefByName {
                    name: "Alias".into(),
                },
            ),
            (
                TypeExpr::TypeParameter(TypeParam {
                    name: "T".into(),
                    constraint: None,
                    default: None,
                }),
                ClosednessRecipe::ParamRef { name: "T".into() },
            ),
            (
                TypeExpr::Function(Arc::new(verter_type_expr::FunctionExpr::synthetic(
                    Vec::new(),
                    None,
                    Vec::new(),
                ))),
                ClosednessRecipe::OpenLeaf,
            ),
            (
                TypeExpr::Unknown { raw: "??".into() },
                ClosednessRecipe::Unsupported,
            ),
        ];
        for (body, expected) in cases {
            let fact = collect_key_domain_closedness_fact(&ident(), &[body], false);
            assert_eq!(&*fact.body_recipes, std::slice::from_ref(&expected));
            assert!(!fact.closed_object_shape);
        }
    }

    #[test]
    fn parenthesized_layers_are_transparent() {
        let body =
            TypeExpr::Parenthesized(Arc::new(TypeExpr::Parenthesized(Arc::new(object_body()))));
        let fact = collect_key_domain_closedness_fact(&ident(), &[body], false);
        assert!(fact.closed_object_shape);
        assert_eq!(&*fact.body_recipes, &[ClosednessRecipe::ObjectClosed]);
    }

    #[test]
    fn empty_body_set_is_no_shape_and_no_recipes() {
        let fact = collect_key_domain_closedness_fact(&ident(), &[], false);
        assert!(!fact.closed_object_shape);
        assert!(fact.body_recipes.is_empty());
    }
}

#[cfg(test)]
mod no_type_expr_poison_asserts {
    //! Compile-time witnesses for the `NoTypeExpr` invariant on the prepared /
    //! type-eval surface. The prepared declaration carriers AND the type-eval
    //! symbol-table inventory are narrowed to facts + content-free locators, so
    //! every stored carrier (and every scalar they reuse) carries the derive —
    //! a reintroduced `TypeExpr` field anywhere in their reachable field graph
    //! fails these asserts at compile time. The transient lowering carriers
    //! that DO hold typed IR live in `type_eval_build` (`Lowered*Parts`) as
    //! producer-local return values only — never stored on the inventory or a
    //! prepared declaration.
    use super::{
        DeclProvenance, PreparedCacheDeps, PreparedExternalDep, PreparedTypeDecl, PreparedValueDecl,
    };
    use crate::analysis::type_eval::{
        EnumMemberValue, EvalEnv, FunctionSignature, MergedTypeBody, TypeDeclBody, TypeDeclGroup,
        TypeDeclInfo, TypeDeclKind, ValueDeclGroup, ValueDeclInfo, ValueDeclKind,
    };
    use crate::analysis::type_solver::host::ResolvedRootIdentity;
    use static_assertions::assert_impl_all;
    use verter_no_typeexpr::NoTypeExpr;

    // The narrowed prepared carriers: fully `TypeExpr`-free, field-recursively.
    assert_impl_all!(PreparedTypeDecl: NoTypeExpr);
    assert_impl_all!(PreparedValueDecl: NoTypeExpr);

    // The TypeExpr-free scalars the carriers reuse verbatim.
    assert_impl_all!(DeclProvenance: NoTypeExpr);
    assert_impl_all!(PreparedCacheDeps: NoTypeExpr);
    assert_impl_all!(PreparedExternalDep: NoTypeExpr);
    assert_impl_all!(TypeDeclKind: NoTypeExpr);
    assert_impl_all!(ValueDeclKind: NoTypeExpr);
    assert_impl_all!(ResolvedRootIdentity: NoTypeExpr);

    // The narrowed type-eval inventory cluster: the whole stored symbol-table
    // surface is `TypeExpr`-free, field-recursively — the signature carrier is
    // the shared closed fact, and the enum member value is the rail-explicit
    // scalar/domain view.
    assert_impl_all!(EvalEnv: NoTypeExpr);
    assert_impl_all!(TypeDeclInfo: NoTypeExpr);
    assert_impl_all!(TypeDeclGroup: NoTypeExpr);
    assert_impl_all!(TypeDeclBody: NoTypeExpr);
    assert_impl_all!(MergedTypeBody: NoTypeExpr);
    assert_impl_all!(ValueDeclInfo: NoTypeExpr);
    assert_impl_all!(ValueDeclGroup: NoTypeExpr);
    assert_impl_all!(FunctionSignature: NoTypeExpr);
    assert_impl_all!(EnumMemberValue: NoTypeExpr);
}
