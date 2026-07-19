//! Shallow declaration symbol-table inventory ([`EvalEnv`]) — the per-file
//! index of declared type and value symbols.
//!
//! An [`EvalEnv`] holds, per declared name, an ordered same-name contributor
//! group carrying content-free FACTS and LOCATORS: declaration kinds,
//! type-parameter header facts, authored-body slot locators, direct
//! member-header facts, narrowed function-signature facts, object-shape facts,
//! and enum member inventories. It stores NO `TypeExpr` — declaration bodies
//! stay at their authored positions and are lowered on demand by the shared
//! resolver through the body locators this inventory records.
//!
//! # Design
//!
//! - **Type symbols**: interfaces, type aliases, classes — each an ordered
//!   [`TypeDeclGroup`] of [`TypeDeclInfo`] contributors in source/binder order.
//! - **Value symbols**: functions, constants, classes, enums — each an ordered
//!   [`ValueDeclGroup`] of [`ValueDeclInfo`] contributors.
//! - **Augmentation scopes**: `declare module "X"` / `declare global` inner
//!   declarations, retained in a separate scoped inventory (never file scope).
//!
//! Same-name declaration merging is represented by the ordered groups; the
//! merge-aware carriers ([`TypeDeclBody`], the merged enum accessors) compose
//! per-contributor locators/facts — they never evaluate a body here.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::analysis::top_level_owners::DeclMap;

use verter_type_expr::facts::{
    EnumMemberEntry, EnumMemberFact, EnumMemberNamesFact, EnumPrimitiveDomain, EnumScalar,
    FunctionSignatureFact, MemberHeaderFact, MemberReturnInferenceFact, ObjectShapeFact,
    ReturnInferenceCompleteness, TypeParamDeclFact, ValueTypeAnnotationFact,
};
use verter_type_expr::locators::{TypeBodyPathStep, TypeBodySlot};
use verter_type_expr::span_origins::FunctionSpansOrigin;
use verter_type_expr::{DeclKey, TopLevelOwnerId};

pub type DeclarationId = u64;

/// The narrowed function-signature carrier of the value inventory: the shared
/// closed [`FunctionSignatureFact`] (parameter/return positions are content-free
/// body locators; spans are recovered via producer-emitted origin locators).
pub type FunctionSignature = FunctionSignatureFact;

// ---------------------------------------------------------------------------
// Symbol table types
// ---------------------------------------------------------------------------

/// A type declaration in the inventory's symbol table.
///
/// Carries the declaration HEADER facts plus the content-free locator of its
/// authored body — never the body itself.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct TypeDeclInfo {
    pub name: String,
    pub owner: TopLevelOwnerId,
    pub declaration_id: DeclarationId,
    pub kind: TypeDeclKind,
    /// Type-parameter header facts: names, ordinals, and (for decl-header
    /// parameters) the content-free locators of their authored constraint /
    /// default bound positions.
    pub type_parameters: TypeParamDeclFact,
    /// Content-free locator of this contributor's authored declaration body
    /// (the whole-body slot: empty path at the declaration's own anchor).
    ///
    /// For a contributor stored in an AUGMENTATION scope, the slot pairs with
    /// its inventory scope key — a scope-aware reader composes the two into the
    /// scoped augmentation locator; the slot itself stays scope-free because
    /// the scoped inventory map is the scope authority.
    pub body: TypeBodySlot,
    /// The declaration's DIRECT syntactic member-header facts (own members
    /// only — heritage contributes nothing here), producer-emitted where the
    /// declaration lowers. Header consumers (the seeded
    /// [`DeclHeaderIndex`](crate::analysis::decl_headers::DeclHeaderIndex)
    /// mirror) read THIS inventory; they never walk a body.
    pub direct_member_headers: Arc<[MemberHeaderFact]>,
    /// Exact, non-deduplicated return-inference facts for authored methods.
    /// Detached `FunctionExpr` values are never used as a lookup key.
    pub direct_member_return_inference: Arc<[MemberReturnInferenceFact]>,
}

impl TypeDeclInfo {
    /// Read one method verdict by its declaration contributor and produced
    /// member path. No name/span/shape rematching is permitted.
    #[must_use]
    pub fn return_inference_for_member(
        &self,
        origin: &FunctionSpansOrigin,
    ) -> Option<ReturnInferenceCompleteness> {
        self.direct_member_return_inference
            .iter()
            .find(|fact| &fact.origin == origin)
            .map(|fact| fact.return_inference)
    }

    /// Read one method verdict by the produced member path after the caller has
    /// already selected this exact declaration contributor. This is a
    /// contributor-local join: it never rematches a member name or searches a
    /// sibling declaration/symbol space.
    #[must_use]
    pub fn return_inference_for_member_path(
        &self,
        member_path: &[u32],
    ) -> Option<ReturnInferenceCompleteness> {
        self.direct_member_return_inference
            .iter()
            .find(|fact| match &fact.origin {
                FunctionSpansOrigin::Member {
                    member_path: candidate,
                    ..
                } => candidate.as_ref() == member_path,
                FunctionSpansOrigin::AliasBody { .. } | FunctionSpansOrigin::Synthetic(_) => false,
            })
            .map(|fact| fact.return_inference)
    }
}

/// An ordered group of same-name type declaration contributors, in
/// source/binder order (append-only).
///
/// Every contributor is retained so [`merged_body`](Self::merged_body) can
/// compose real TypeScript declaration merging (interface+interface and
/// interface+class fold into a [`TypeDeclBody::Merged`] carrier of
/// per-contributor body locators).
/// [`primary`](Self::primary) returns the LAST contributor — the
/// last-wins representative used where a single declaration is required (alias
/// groups, single-contributor groups).
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct TypeDeclGroup {
    /// Contributors in source/binder order. Always non-empty once created.
    pub contributors: Vec<TypeDeclInfo>,
}

impl TypeDeclGroup {
    /// Create a group seeded with a single contributor.
    pub fn new(decl: TypeDeclInfo) -> Self {
        Self {
            contributors: vec![decl],
        }
    }

    /// The authoritative contributor under today's last-wins semantics: the
    /// LAST one appended.
    pub fn primary(&self) -> &TypeDeclInfo {
        self.contributors
            .last()
            .expect("TypeDeclGroup is never empty")
    }

    /// Mutable access to the last (authoritative) contributor.
    pub fn primary_mut(&mut self) -> &mut TypeDeclInfo {
        self.contributors
            .last_mut()
            .expect("TypeDeclGroup is never empty")
    }

    /// All contributors in source/binder order.
    pub fn contributors(&self) -> &[TypeDeclInfo] {
        &self.contributors
    }

    /// The union of DIRECT member-header facts across every contributor, in
    /// first-seen order — the stored-fact shallow-index view (never a body
    /// walk). Matches the production header index's first-seen member union
    /// (`upsert_type_header`), so a seeded header index reconstructs the same
    /// member headers the parse-time walk records.
    pub fn merged_member_header_facts(&self) -> Vec<MemberHeaderFact> {
        let mut out: Vec<MemberHeaderFact> = Vec::new();
        for decl in &self.contributors {
            for fact in decl.direct_member_headers.iter() {
                if !out.iter().any(|existing| existing.name == fact.name) {
                    out.push(fact.clone());
                }
            }
        }
        out
    }

    /// Produce the merge-aware declaration-body LOCATOR carrier for this group.
    ///
    /// Multiple same-name `interface` declarations in one scope are merged by
    /// TypeScript: their members union and same-name methods accumulate into an
    /// ordered overload group. An `interface` + `class` group ALSO merges — the
    /// interface members augment the class INSTANCE type (the class's
    /// value / static / constructor side lives on a SEPARATE value declaration,
    /// untouched by this type-side merge). Such a group lowers to a
    /// [`TypeDeclBody::Merged`] carrier of PER-CONTRIBUTOR body slots (each
    /// addressing one ordered contributor of the merged declaration) so the
    /// project-semantic reducer can peer-merge the lowered contributors (NOT a
    /// bare intersection, which would heritage-shadow).
    ///
    /// A type `alias` never merges (a duplicate-identifier error in TS); any
    /// group containing an alias — or any single-contributor group — keeps
    /// today's last-wins [`TypeDeclBody::Single`] whole-body slot.
    pub fn merged_body(&self) -> TypeDeclBody {
        self.merged_body_dual_space(false)
    }

    /// [`Self::merged_body`] with the caller's DUAL-SPACE knowledge.
    ///
    /// An `enum` registers its type-space contributor as the `Alias` it
    /// structurally is (there is no dedicated enum `TypeDeclKind`), so the
    /// kind-based mergeable predicate alone cannot see that same-name `enum`
    /// declarations DO merge in TypeScript. The caller that holds BOTH
    /// spaces — the fold that derives the enum's projected-type union from
    /// the VALUE sibling (see [`ValueDeclGroup::enum_type_union`]) — passes
    /// `is_enum = true`, and a multi-contributor group keeps EVERY
    /// contributor body slot instead of collapsing last-wins (a last-wins
    /// fold would leave the earlier declarations' bodies unaddressable).
    pub fn merged_body_dual_space(&self, is_enum: bool) -> TypeDeclBody {
        // `interface`+`interface` and `interface`+`class` are the two valid
        // same-name type merges (plus `enum`+`enum`, visible only through the
        // caller's dual-space `is_enum` knowledge above). Two same-name
        // `class` declarations are a duplicate-identifier ERROR in TypeScript;
        // folding such a (malformed) group into a `Merged` carrier is benign —
        // it produces a union of the two class instance bodies for invalid
        // input rather than special-casing it, which keeps the predicate
        // simple and never affects well-formed code.
        let all_mergeable = is_enum
            || self
                .contributors
                .iter()
                .all(|decl| matches!(decl.kind, TypeDeclKind::Interface | TypeDeclKind::Class));
        if self.contributors.len() > 1 && all_mergeable {
            TypeDeclBody::Merged(MergedTypeBody {
                contributors: self
                    .contributors
                    .iter()
                    .enumerate()
                    .filter_map(|(ordinal, decl)| {
                        Some(TypeBodySlot {
                            anchor: decl.body.anchor.clone(),
                            path: Arc::from([TypeBodyPathStep::MergedContributor {
                                ordinal: u32::try_from(ordinal).ok()?,
                            }]),
                        })
                    })
                    .collect(),
                kinds: self.contributors.iter().map(|d| d.kind).collect(),
            })
        } else {
            TypeDeclBody::Single(self.primary().body.clone())
        }
    }
}

/// The body LOCATOR carrier of a type declaration, carrying same-file
/// declaration-merge provenance.
///
/// [`Single`](Self::Single) is the non-merged path — the one contributor's
/// whole-body slot. [`Merged`](Self::Merged) carries every same-name
/// `interface` contributor's body SLOT in source order (each addressing one
/// ordered merged contributor) so the project-semantic reducer can lower and
/// peer-merge them into one surface (member union + ordered method overload
/// groups). A merged declaration MUST reach the reducer as this distinct
/// carrier; collapsing it to a bare intersection would route it through
/// heritage-shadow member semantics — the wrong rule.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub enum TypeDeclBody {
    /// A single declaration's whole-body slot.
    Single(TypeBodySlot),
    /// Multiple same-name interface contributors' slots, in source order.
    Merged(MergedTypeBody),
}

/// The ordered contributor body slots + kinds of a merged declaration.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct MergedTypeBody {
    /// Contributor body slots in source/binder order (each addresses one
    /// ordered merged contributor of the declaration).
    pub contributors: Vec<TypeBodySlot>,
    /// Contributor kinds, parallel to [`contributors`](Self::contributors).
    pub kinds: Vec<TypeDeclKind>,
}

impl TypeDeclBody {
    /// Construct a non-merged single body slot.
    pub fn single(body: TypeBodySlot) -> Self {
        Self::Single(body)
    }

    /// Whether this body carries more than one merged contributor.
    pub fn is_merged(&self) -> bool {
        matches!(self, Self::Merged(_))
    }

    /// Every contributor body slot in source order (one element for `Single`).
    pub fn contributors(&self) -> &[TypeBodySlot] {
        match self {
            Self::Single(body) => std::slice::from_ref(body),
            Self::Merged(merged) => &merged.contributors,
        }
    }

    /// The last-wins representative contributor body slot (the final
    /// declaration).
    pub fn primary(&self) -> &TypeBodySlot {
        self.contributors()
            .last()
            .expect("TypeDeclBody always has at least one contributor")
    }
}

/// What kind of type declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, verter_no_typeexpr::NoTypeExpr)]
pub enum TypeDeclKind {
    Alias,
    Interface,
    Class,
}

/// The classified value of one `enum` member.
///
/// An enum member always has a NAME (recorded on the presence rail). Its VALUE
/// is either statically FOLDED to a literal scalar, or DEFERRED — a computed /
/// expression / member-reference initializer the literal-enum producer does not
/// constant-fold (`B = 1 << 2`, `B = someFn()`, `~A`, or a bare member after an
/// unknown running value). A deferred member is NEVER dropped and NEVER given a
/// fabricated literal: it carries the narrowest SOUND primitive DOMAIN proven
/// from its initializer-expression KIND, so every type/value projection surface
/// (`typeof Enum`, `keyof typeof Enum`, `Enum.Member`, the enum type union) sees
/// the member with an honest, never-under-approximating type.
///
/// This is the rail-explicit VIEW over the stored [`EnumScalar`]: the two are a
/// lossless bijection ([`from_scalar`](Self::from_scalar) /
/// [`projected_scalar`](Self::projected_scalar)) — a folded member maps to the
/// `Number` / `String` scalar arms, a deferred one to the `Primitive` domain
/// arm.
#[derive(Debug, Clone, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub enum EnumMemberValue {
    /// Statically folded to a literal scalar (string / ±numeric /
    /// auto-increment). This literal is BOTH the projected type AND the
    /// value-body fingerprint basis — the only members the foldable rail
    /// observes. Producer invariant: the scalar is always a `Number` / `String`
    /// arm, never `Primitive` (a primitive domain is the deferred arm's
    /// payload).
    Folded(EnumScalar),
    /// Value deferred. Carries the narrowest SOUND primitive domain proven from
    /// the initializer-expression kind — never the enum self-reference and
    /// NEVER a fabricated literal. The member's NAME stays on the presence
    /// rail; only its foldable VALUE is absent (it is projected out of the
    /// fingerprint, NOT out of the type surfaces).
    Deferred(EnumPrimitiveDomain),
}

impl EnumMemberValue {
    /// Classify a stored member scalar back into its rail-explicit view: a
    /// `Number` / `String` scalar is a folded member, a `Primitive` domain is a
    /// deferred one. The inverse of [`projected_scalar`](Self::projected_scalar).
    pub fn from_scalar(scalar: &EnumScalar) -> Self {
        match scalar {
            EnumScalar::Number(_) | EnumScalar::String(_) => Self::Folded(scalar.clone()),
            EnumScalar::Primitive(domain) => Self::Deferred(*domain),
        }
    }

    /// The scalar this member PROJECTS to on every type/value surface (`typeof
    /// Enum`, `Enum.Member`, the enum type union): the folded literal for a
    /// foldable member, or the degraded primitive domain for a deferred one.
    /// ALWAYS present — a deferred member is degraded, never dropped. This is
    /// also the STORED form ([`EnumMemberEntry::value`]).
    pub fn projected_scalar(&self) -> EnumScalar {
        match self {
            Self::Folded(scalar) => scalar.clone(),
            Self::Deferred(domain) => EnumScalar::Primitive(*domain),
        }
    }

    /// The folded literal scalar, or `None` when the member's value is
    /// deferred. This is the foldable-only view the value-body fingerprint
    /// observes — a deferred member MUST NOT enter the fingerprint, because its
    /// degraded domain is not a value edit.
    pub fn folded_literal(&self) -> Option<&EnumScalar> {
        match self {
            Self::Folded(scalar) => Some(scalar),
            Self::Deferred(_) => None,
        }
    }
}

/// A value declaration in the inventory's value symbol table.
///
/// Carries narrowed FACTS only: the annotation fact (classification + typeof
/// peel target + source), signature facts, object-shape facts, and the enum
/// member inventory. Authored positions are content-free locators; bodies
/// lower on demand through the shared resolver.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct ValueDeclInfo {
    pub name: String,
    pub owner: TopLevelOwnerId,
    pub declaration_id: DeclarationId,
    pub kind: ValueDeclKind,
    /// The narrowed annotation FACT: classification ([`Absent`/`Direct`/
    /// `TypeOfAlias`](verter_type_expr::facts::ValueAnnotationClass)), the
    /// precomputed single-hop `typeof x` peel target, and (when derivable) the
    /// annotation source.
    pub type_annotation: ValueTypeAnnotationFact,
    /// Narrowed function/method signature facts. Empty = non-callable; length
    /// 1 = the common single-declaration case; length > 1 = an overload group
    /// (source order; the trailing entry may be the implementation, flagged by
    /// `has_implementation_body`). Parameter/return positions are content-free
    /// locators of the authored positions; their leading
    /// [`ValueSignature`](verter_type_expr::locators::TypeBodyPathStep::ValueSignature)
    /// ordinal is the GROUP-level overload ordinal (kept correct across
    /// same-name contributor appends by [`EvalEnv::add_value`]).
    pub signatures: Vec<FunctionSignatureFact>,
    /// Narrowed object-shape fact, if this is a const initialized with an
    /// object (or a class's `typeof C` constructor-object shape). Member value
    /// positions are content-free locators whose `Member` ordinals index THIS
    /// produced shape surface in source order (the shape is recovered by
    /// re-lowering the declaration on demand).
    pub object_shape: Option<ObjectShapeFact>,
    /// The ordered member inventory of a [`ValueDeclKind::Enum`] declaration,
    /// in SOURCE declaration order (TS enum members are ordered — the
    /// auto-increment of a bare member depends on the preceding member, and the
    /// `typeof Enum` object surfaces members in declaration order).
    ///
    /// This is the SINGLE source of truth for BOTH enum rails, so the two can
    /// never diverge by construction: the member NAME is recorded for EVERY
    /// statically-named member (all four `TSEnumMemberName` variants resolve to
    /// a static name via the SAME `static_name` helper the production
    /// `index_enum` header walk uses), while each member's stored
    /// [`EnumScalar`] is a folded literal (string / ±numeric / auto-increment)
    /// or a deferred member's degraded sound primitive DOMAIN
    /// ([`EnumScalar::Primitive`]) — honest on every surface, never dropped.
    /// The `Folded` subset is therefore an intrinsic subset of the full member
    /// set:
    /// - The NAME rail — [`ValueDeclGroup::merged_enum_member_names_fact`] — is
    ///   the member-presence authority (`enum_headers`, `parse_stable_hash`,
    ///   enum `MemberPresence` facts); it must match `index_enum` exactly.
    /// - The PROJECTION rail — every member's projected scalar — drives the
    ///   `typeof Enum` object, the `Enum.Member` projection, and the enum type
    ///   union (folded literal for foldable members, degraded primitive for
    ///   deferred ones), so NO known member ever vanishes from a type surface.
    /// - The FOLDABLE rail — the [`EnumMemberValue::folded_literal`] subset,
    ///   projected by [`ValueDeclGroup::merged_enum_members`] — is the
    ///   value-body fact fingerprint basis ONLY; a deferred member's degraded
    ///   domain is not a value edit and stays out of it.
    ///
    /// `Some(..)` exactly when this value decl is an enum (possibly an empty
    /// member list); `None` for every non-enum value declaration.
    pub enum_members: Option<EnumMemberFact>,
    /// The enum's member-NAME inventory fact — the full statically-named
    /// member-name superset, producer-emitted from the SAME member walk that
    /// builds [`enum_members`](Self::enum_members) (one derivation point, so
    /// the two cannot diverge). Header consumers (the seeded
    /// [`DeclHeaderIndex`](crate::analysis::decl_headers::DeclHeaderIndex)
    /// mirror's `enum_headers`) read THIS fact; they never walk the value
    /// inventory. `Some` exactly when this value decl is an enum.
    pub enum_member_names: Option<EnumMemberNamesFact>,
}

/// An ordered group of same-name value declaration contributors, in
/// source/binder order (append-only).
///
/// [`merged_signatures`](Self::merged_signatures) concatenates every
/// contributor's signature facts in source order (the function-overload group);
/// [`primary`](Self::primary) returns the LAST contributor, the last-wins
/// representative used where a single declaration is required.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct ValueDeclGroup {
    /// Contributors in source/binder order. Always non-empty once created.
    pub contributors: Vec<ValueDeclInfo>,
}

impl ValueDeclGroup {
    /// Create a group seeded with a single contributor.
    pub fn new(decl: ValueDeclInfo) -> Self {
        Self {
            contributors: vec![decl],
        }
    }

    /// The authoritative contributor under today's last-wins semantics: the
    /// LAST one appended.
    pub fn primary(&self) -> &ValueDeclInfo {
        self.contributors
            .last()
            .expect("ValueDeclGroup is never empty")
    }

    /// Mutable access to the last (authoritative) contributor.
    pub fn primary_mut(&mut self) -> &mut ValueDeclInfo {
        self.contributors
            .last_mut()
            .expect("ValueDeclGroup is never empty")
    }

    /// All contributors in source/binder order.
    pub fn contributors(&self) -> &[ValueDeclInfo] {
        &self.contributors
    }

    /// The merged overload signature-fact set: every contributor's signature
    /// facts concatenated in source order. A function declared with bodiless
    /// overloads followed by an implementation contributes one signature per
    /// declaration, so the returned vector is the full ordered overload group
    /// (the trailing implementation entry carries `has_implementation_body`).
    /// Each fact's leading `ValueSignature` locator ordinal is the GROUP-level
    /// overload ordinal (maintained at append time by [`EvalEnv::add_value`]),
    /// so concatenation preserves locator correctness. For a single contributor
    /// this is exactly its own signature facts.
    pub fn merged_signatures(&self) -> Vec<FunctionSignatureFact> {
        if self.contributors.len() == 1 {
            return self.contributors[0].signatures.clone();
        }
        self.contributors
            .iter()
            .flat_map(|decl| decl.signatures.iter().cloned())
            .collect()
    }

    /// The merged enum member inventory: every contributor's
    /// [`enum_members`](ValueDeclInfo::enum_members) folded in source order, the
    /// SHARED basis both enum rails derive from so they can never disagree on
    /// which contributor owns a member. TS declaration merging lets
    /// `enum E { A }` and a later `enum E { B = 1 }` contribute to one enum, so
    /// the member set is the UNION of all same-name contributors — using only
    /// the last (last-wins `primary()`) contributor would drop the earlier
    /// declarations' members.
    ///
    /// TS forbids a duplicate member NAME across merged enum bodies, so the
    /// collision path is defensive: on a name already seen, the FIRST
    /// contributor's entry (name AND scalar) wins deterministically, so every
    /// derived rail agrees on every member's owning contributor.
    /// Returns `None` when no contributor is an enum.
    pub fn merged_enum_unified(&self) -> Option<EnumMemberFact> {
        let mut is_enum = false;
        let mut merged: Vec<EnumMemberEntry> = Vec::new();
        for decl in &self.contributors {
            let Some(members) = decl.enum_members.as_ref() else {
                continue;
            };
            is_enum = true;
            for entry in members.members.iter() {
                if !merged.iter().any(|existing| existing.name == entry.name) {
                    merged.push(entry.clone());
                }
            }
        }
        is_enum.then(|| EnumMemberFact {
            members: merged.into(),
        })
    }

    /// The FULL ordered member-NAME set of the merged enum — EVERY
    /// statically-named member, including ones whose VALUE is deferred — read
    /// from the stored per-contributor [`EnumMemberNamesFact`] inventory
    /// (first-seen union across contributors; never a value-body walk). This is
    /// the member-presence authority: it must match what the production
    /// `index_enum` header walk records (both resolve names via `static_name`),
    /// so a seeded `DeclHeaderIndex` reconstructs `enum_headers` identically and
    /// the enum `MemberPresence` facts / `parse_stable_hash` enum-header fold
    /// stay correct for seeded artifacts. Returns `None` when no contributor is
    /// an enum.
    pub fn merged_enum_member_names_fact(&self) -> Option<EnumMemberNamesFact> {
        let mut is_enum = false;
        let mut names: Vec<String> = Vec::new();
        for decl in &self.contributors {
            let Some(fact) = decl.enum_member_names.as_ref() else {
                continue;
            };
            is_enum = true;
            for name in fact.names.iter() {
                if !names.iter().any(|existing| existing == name) {
                    names.push(name.clone());
                }
            }
        }
        is_enum.then(|| EnumMemberNamesFact {
            names: names.into(),
        })
    }

    /// The merged enum's FOLDABLE rail: every member with a statically known
    /// value-literal scalar, in source order. Members whose value is DEFERRED
    /// are projected out (their degraded domain is not a folded literal), so
    /// this rail is reserved for the value-body fact fingerprint — the ONLY
    /// consumer that must observe foldable members alone. Type/value PROJECTION
    /// surfaces (`typeof Enum`, `Enum.Member`,
    /// [`enum_type_union`](Self::enum_type_union)) read the FULL member set via
    /// each member's projected scalar instead, so a deferred member is
    /// degraded, never dropped. Returns `None` when no contributor is an enum.
    pub fn merged_enum_members(&self) -> Option<Vec<(String, EnumScalar)>> {
        self.merged_enum_unified().map(|merged| {
            merged
                .members
                .iter()
                .filter_map(|entry| {
                    EnumMemberValue::from_scalar(&entry.value)
                        .folded_literal()
                        .cloned()
                        .map(|scalar| (entry.name.clone(), scalar))
                })
                .collect()
        })
    }

    /// The enum's TYPE-space body arms: the deduplicated UNION of EVERY
    /// member's projected scalar, derived from the SAME full merged member set
    /// the name rail derives from. An `enum` is dual-space — a VALUE (its
    /// `typeof` object / `Enum.Member` projection) and a TYPE (the union used
    /// when the enum names a type) — and BOTH spaces carry exactly the same
    /// member set so they can never diverge.
    ///
    /// Honesty floor: a deferred member contributes its DEGRADED sound
    /// primitive arm, so a NON-EMPTY enum NEVER collapses to the empty arm set
    /// (an all-deferred enum is `number` / `string` / `number | string` /
    /// `unknown`, never empty) and NEVER narrows to the folded subset (a
    /// partial enum keeps both the folded literals and the deferred members'
    /// degraded arms). Distinct arms are deduped, so an all-`number`-degraded
    /// enum yields the single `number` arm rather than `number | number`.
    ///
    /// This is the SINGLE source of truth for the enum type body's arm set —
    /// the graph layer builds the actual union type from these scalar arms on
    /// demand. The per-declaration inventory walk cannot see same-name merged
    /// contributors, so the type-space binding carries only its declaration
    /// locator and this derivation composes the merged arms. Returns `None`
    /// when the group is not an enum; an empty member list yields an empty arm
    /// set (the `never` union at the graph layer).
    pub fn enum_type_union(&self) -> Option<Vec<EnumScalar>> {
        let members = self.merged_enum_unified()?;
        let mut arms: Vec<EnumScalar> = Vec::new();
        for entry in members.members.iter() {
            let arm = EnumMemberValue::from_scalar(&entry.value).projected_scalar();
            if !arms.contains(&arm) {
                arms.push(arm);
            }
        }
        Some(arms)
    }
}

/// What kind of value declaration this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, verter_no_typeexpr::NoTypeExpr)]
pub enum ValueDeclKind {
    Const,
    Let,
    Var,
    Function,
    AsyncFunction,
    Class,
    /// TypeScript enum declaration — dual-space: type (union of members)
    /// and value (object with member lookup).
    Enum,
}

// ---------------------------------------------------------------------------
// Inventory environment
// ---------------------------------------------------------------------------

/// The ambient declaration-augmentation scope an inner declaration belongs to.
///
/// Ambient augmentation blocks (`declare module "X" { ... }` and
/// `declare global { ... }`) do NOT contribute to the file's top-level symbol
/// table — their inner declarations augment a DIFFERENT module's surface (the
/// canonical Vue/Vite `declare module "vue"` pattern) or the global scope.
/// They are retained in a SEPARATE scoped inventory so cross-file augmentation
/// can stitch them onto the augmented declaration on demand, without polluting
/// file-scope `type_symbols`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub enum AugmentationScopeKind {
    /// `declare global { ... }` — augments the global scope.
    Global,
    /// `declare module "<specifier>" { ... }` — augments the module reached by
    /// the RAW specifier as written in the source. The owner crate keeps the
    /// specifier verbatim; the session layer resolves it to a canonical id (for
    /// relative specifiers) when it stitches augmenters through the
    /// augmentation index.
    Module(String),
}

/// The per-file shallow symbol-table inventory: content-free facts + locators
/// per declared symbol, grouped by name in source/binder order.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct EvalEnv {
    /// Type declarations: interfaces, type aliases. Each name maps to an
    /// ordered group of contributors (append-only, source/binder order).
    pub type_symbols: DeclMap<TypeDeclGroup>,
    /// Value declarations: functions, constants, classes. Each name maps to
    /// an ordered group of contributors (append-only, source/binder order).
    pub value_symbols: DeclMap<ValueDeclGroup>,
    /// Ambient declaration-augmentation inventory: `(scope, name)` → ordered
    /// contributor group. Holds the retained INDEX entries of declarations
    /// nested in `declare module "X" { ... }` / `declare global { ... }` blocks
    /// so a scoped declaration lookup can address them. Kept separate from
    /// `type_symbols` — these inner declarations never enter the file's
    /// top-level surface.
    pub augmentation_scopes: FxHashMap<(AugmentationScopeKind, DeclKey), TypeDeclGroup>,
    /// Value-space counterpart to [`augmentation_scopes`](Self::augmentation_scopes):
    /// the retained INDEX entries of VALUE declarations (`const`/`let`/`var`,
    /// `function`, `class`, `enum`) nested in `declare module "X" { ... }` /
    /// `declare global { ... }` blocks. Kept separate from `value_symbols`
    /// (file scope) — these augment another module's value surface and are the
    /// typed source for value-space module-augmentation facts.
    pub augmentation_value_scopes: FxHashMap<(AugmentationScopeKind, DeclKey), ValueDeclGroup>,
    /// Stable ids assigned to type declarations inserted into this environment.
    type_decl_ids: FxHashMap<DeclKey, DeclarationId>,
    /// Stable ids assigned to value declarations inserted into this environment.
    value_decl_ids: FxHashMap<DeclKey, DeclarationId>,
    /// Traversal budgets for consumers that walk this inventory.
    pub limits: EvalLimits,
    /// Total traversal steps consumed (monotonically increasing).
    steps: usize,
    /// Monotonic declaration ordinal used to assign stable ids.
    next_declaration_id: DeclarationId,
    /// Preserve canonical vue `VNode` slot return types symbolically while
    /// still normalizing other slot return types during defineSlots expansion.
    pub preserve_canonical_vue_vnode_slot_returns: bool,
}

/// Traversal budgets for inventory consumers.
///
/// The inventory itself performs no evaluation; these budgets bound the
/// traversal work of consumers that walk it (the shared resolver's
/// demand-driven lowering and expansion paths), and the step counter
/// ([`EvalEnv::steps`]) is the shared accounting they charge against.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct EvalLimits {
    /// Maximum structural traversal depth. Default: 32.
    pub max_depth: usize,
    /// Maximum union-arm expansion width. Default: 64.
    pub max_union_expansion: usize,
    /// Maximum mapped-type key enumeration width. Default: 128.
    pub max_mapped_keys: usize,
    /// Maximum nested mapped-type traversal depth. Default: 3.
    pub max_mapped_depth: usize,
    /// Safety-net total step budget. Default: 50_000.
    pub max_steps: usize,
    /// Maximum reference-chain follow depth. Default: 8.
    pub max_ref_depth: usize,
}

impl Default for EvalLimits {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_union_expansion: 64,
            max_mapped_keys: 128,
            max_mapped_depth: 3,
            max_steps: 50_000,
            max_ref_depth: 8,
        }
    }
}

impl EvalEnv {
    /// Total number of declaration CONTRIBUTORS this environment holds across
    /// the file-scope type/value tables and the augmentation-scope
    /// inventories. Each contributor corresponds to one indexed declaration (a
    /// same-name merge group of `k` declarations counts `k`). This is the
    /// deterministic per-declaration measure the host's demand-scoping
    /// counters observe when a whole-file environment is built.
    #[must_use]
    pub fn total_decl_count(&self) -> usize {
        self.type_symbols
            .values()
            .map(|g| g.contributors().len())
            .sum::<usize>()
            + self
                .value_symbols
                .values()
                .map(|g| g.contributors().len())
                .sum::<usize>()
            + self
                .augmentation_scopes
                .values()
                .map(|g| g.contributors().len())
                .sum::<usize>()
            + self
                .augmentation_value_scopes
                .values()
                .map(|g| g.contributors().len())
                .sum::<usize>()
    }

    /// Create a new inventory environment with default limits.
    pub fn new() -> Self {
        Self {
            type_symbols: DeclMap::default(),
            value_symbols: DeclMap::default(),
            augmentation_scopes: FxHashMap::default(),
            augmentation_value_scopes: FxHashMap::default(),
            type_decl_ids: FxHashMap::default(),
            value_decl_ids: FxHashMap::default(),
            limits: EvalLimits::default(),
            steps: 0,
            next_declaration_id: 0,
            preserve_canonical_vue_vnode_slot_returns: false,
        }
    }

    /// Create an environment with custom limits.
    pub fn with_limits(limits: EvalLimits) -> Self {
        Self {
            limits,
            ..Self::new()
        }
    }

    #[must_use]
    pub fn type_group(&self, name: &str) -> Option<&TypeDeclGroup> {
        self.type_group_in(TopLevelOwnerId::ordinary_file(), name)
    }

    #[must_use]
    pub fn type_group_in(&self, owner: TopLevelOwnerId, name: &str) -> Option<&TypeDeclGroup> {
        self.type_symbols.get(&DeclKey::new(owner, name))
    }

    #[must_use]
    pub fn value_group(&self, name: &str) -> Option<&ValueDeclGroup> {
        self.value_group_in(TopLevelOwnerId::ordinary_file(), name)
    }

    #[must_use]
    pub fn value_group_in(&self, owner: TopLevelOwnerId, name: &str) -> Option<&ValueDeclGroup> {
        self.value_symbols.get(&DeclKey::new(owner, name))
    }

    /// Register a type declaration, appending it to the named group in
    /// source/binder order (creating the group if absent).
    pub fn add_type(&mut self, mut decl: TypeDeclInfo) {
        let key = DeclKey::new(decl.owner, decl.name.as_str());
        let decl_id = self.stabilize_type_declaration_id(&key, decl.declaration_id);
        decl.declaration_id = decl_id;
        match self.type_symbols.get_mut(&key) {
            Some(group) => group.contributors.push(decl),
            None => {
                self.type_symbols.insert(key, TypeDeclGroup::new(decl));
            }
        }
    }

    /// Register a value declaration, appending it to the named group in
    /// source/binder order (creating the group if absent).
    ///
    /// Appending REBASES the declaration's leading `ValueSignature` locator
    /// ordinals to the GROUP-level overload positions (the number of signatures
    /// contributed by prior same-name contributors), so a stored signature
    /// fact's parameter/return locators always address the merged overload
    /// group's ordinal — deterministic and idempotent under order-preserving
    /// re-registration ([`extend_missing`](Self::extend_missing)).
    pub fn add_value(&mut self, mut decl: ValueDeclInfo) {
        let key = DeclKey::new(decl.owner, decl.name.as_str());
        let decl_id = self.stabilize_value_declaration_id(&key, decl.declaration_id);
        decl.declaration_id = decl_id;
        match self.value_symbols.get_mut(&key) {
            Some(group) => {
                match checked_signature_base(&group.contributors) {
                    Some(base) if rebase_value_signature_ordinals(&mut decl, base) => {}
                    _ => decl.signatures.clear(),
                }
                group.contributors.push(decl);
            }
            None => {
                self.value_symbols.insert(key, ValueDeclGroup::new(decl));
            }
        }
    }

    /// Register a type declaration nested in an ambient augmentation block
    /// (`declare module "X"` / `declare global`), appending it to the named
    /// group inside the augmentation scope (creating the group if absent).
    /// These declarations are retained for cross-file augmentation stitching
    /// and never enter the file-scope `type_symbols`.
    pub fn add_augmentation_type(&mut self, scope: AugmentationScopeKind, decl: TypeDeclInfo) {
        let key = DeclKey::new(decl.owner, decl.name.as_str());
        match self
            .augmentation_scopes
            .get_mut(&(scope.clone(), key.clone()))
        {
            Some(group) => group.contributors.push(decl),
            None => {
                self.augmentation_scopes
                    .insert((scope, key), TypeDeclGroup::new(decl));
            }
        }
    }

    /// Register a VALUE declaration nested in an ambient augmentation block,
    /// appending it to the named group inside the augmentation value scope
    /// (creating the group if absent; same `ValueSignature` ordinal rebase as
    /// [`add_value`](Self::add_value)). Retained for value-space cross-file
    /// augmentation facts; never enters file-scope `value_symbols`.
    pub fn add_augmentation_value(
        &mut self,
        scope: AugmentationScopeKind,
        mut decl: ValueDeclInfo,
    ) {
        let key = DeclKey::new(decl.owner, decl.name.as_str());
        match self
            .augmentation_value_scopes
            .get_mut(&(scope.clone(), key.clone()))
        {
            Some(group) => {
                match checked_signature_base(&group.contributors) {
                    Some(base) if rebase_value_signature_ordinals(&mut decl, base) => {}
                    _ => decl.signatures.clear(),
                }
                group.contributors.push(decl);
            }
            None => {
                self.augmentation_value_scopes
                    .insert((scope, key), ValueDeclGroup::new(decl));
            }
        }
    }

    /// Returns the total number of traversal steps consumed so far.
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// Returns whether the step budget has been exhausted.
    pub fn budget_exhausted(&self) -> bool {
        self.steps >= self.limits.max_steps
    }

    /// Merge declarations from another environment without overwriting
    /// declarations already present in `self`.
    pub fn extend_missing(&mut self, other: EvalEnv) {
        self.extend_missing_from_ref(&other);
    }

    /// Merge declarations from another environment by reference without
    /// cloning the full environment up front.
    pub fn extend_missing_from_ref(&mut self, other: &EvalEnv) {
        for (key, group) in &other.type_symbols {
            if !self.type_symbols.contains_key(key) {
                for decl in group.contributors() {
                    self.add_type(decl.clone());
                }
            }
        }
        for (key, group) in &other.value_symbols {
            if !self.value_symbols.contains_key(key) {
                for decl in group.contributors() {
                    self.add_value(decl.clone());
                }
            }
        }
        for (key, decl_id) in &other.type_decl_ids {
            if *decl_id == 0 {
                continue;
            }
            let stable_id = self.stabilize_type_declaration_id(key, *decl_id);
            if let Some(group) = self.type_symbols.get_mut(key) {
                let decl = group.primary_mut();
                if decl.declaration_id == 0 {
                    decl.declaration_id = stable_id;
                }
            }
        }
        for (key, decl_id) in &other.value_decl_ids {
            if *decl_id == 0 {
                continue;
            }
            let stable_id = self.stabilize_value_declaration_id(key, *decl_id);
            if let Some(group) = self.value_symbols.get_mut(key) {
                let decl = group.primary_mut();
                if decl.declaration_id == 0 {
                    decl.declaration_id = stable_id;
                }
            }
        }
        self.next_declaration_id = self.next_declaration_id.max(other.next_declaration_id);
    }

    pub fn type_declaration_id(&self, name: &str) -> Option<DeclarationId> {
        self.type_declaration_id_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn type_declaration_id_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<DeclarationId> {
        self.type_decl_ids.get(&DeclKey::new(owner, name)).copied()
    }

    pub fn value_declaration_id(&self, name: &str) -> Option<DeclarationId> {
        self.value_declaration_id_in(TopLevelOwnerId::ordinary_file(), name)
    }

    pub fn value_declaration_id_in(
        &self,
        owner: TopLevelOwnerId,
        name: &str,
    ) -> Option<DeclarationId> {
        self.value_decl_ids.get(&DeclKey::new(owner, name)).copied()
    }

    fn stabilize_type_declaration_id(
        &mut self,
        key: &DeclKey,
        declaration_id: DeclarationId,
    ) -> DeclarationId {
        if declaration_id != 0 {
            let stable_id = *self
                .type_decl_ids
                .entry(key.clone())
                .or_insert(declaration_id);
            self.next_declaration_id = self.next_declaration_id.max(stable_id);
            stable_id
        } else if let Some(existing) = self.type_decl_ids.get(key).copied() {
            existing
        } else {
            let decl_id = self.allocate_declaration_id();
            self.type_decl_ids.insert(key.clone(), decl_id);
            decl_id
        }
    }

    fn stabilize_value_declaration_id(
        &mut self,
        key: &DeclKey,
        declaration_id: DeclarationId,
    ) -> DeclarationId {
        if declaration_id != 0 {
            let stable_id = *self
                .value_decl_ids
                .entry(key.clone())
                .or_insert(declaration_id);
            self.next_declaration_id = self.next_declaration_id.max(stable_id);
            stable_id
        } else if let Some(existing) = self.value_decl_ids.get(key).copied() {
            existing
        } else {
            let decl_id = self.allocate_declaration_id();
            self.value_decl_ids.insert(key.clone(), decl_id);
            decl_id
        }
    }

    fn allocate_declaration_id(&mut self) -> DeclarationId {
        self.next_declaration_id += 1;
        self.next_declaration_id
    }
}

impl Default for EvalEnv {
    fn default() -> Self {
        Self::new()
    }
}

/// Re-point a value declaration's leading
/// [`ValueSignature`](TypeBodyPathStep::ValueSignature) locator ordinals at the
/// GROUP-level overload positions: the `j`-th signature fact of this
/// declaration takes ordinal `base + j`. The assignment SETS the ordinal from
/// the fact's position (never adds to the stored value), so re-registering the
/// same contributors in the same order reproduces identical locators.
fn checked_signature_base(contributors: &[ValueDeclInfo]) -> Option<u32> {
    contributors.iter().try_fold(0u32, |total, contributor| {
        total.checked_add(u32::try_from(contributor.signatures.len()).ok()?)
    })
}

fn checked_rebased_ordinal(base: u32, index: usize) -> Option<u32> {
    base.checked_add(u32::try_from(index).ok()?)
}

fn rebase_value_signature_ordinals(decl: &mut ValueDeclInfo, base: u32) -> bool {
    if let Some(last) = decl.signatures.len().checked_sub(1) {
        let Ok(last) = u32::try_from(last) else {
            return false;
        };
        if base.checked_add(last).is_none() {
            return false;
        }
    }
    for (j, signature) in decl.signatures.iter_mut().enumerate() {
        let Some(ordinal) = checked_rebased_ordinal(base, j) else {
            return false;
        };
        let repoint = |slot: &mut TypeBodySlot| {
            if let Some(TypeBodyPathStep::ValueSignature { .. }) = slot.path.first() {
                let mut path: Vec<TypeBodyPathStep> = slot.path.to_vec();
                path[0] = TypeBodyPathStep::ValueSignature { ordinal };
                slot.path = path.into();
            }
        };
        if let Some(return_ty) = &mut signature.return_ty {
            repoint(return_ty);
        }
        let mut parameters = signature.parameters.to_vec();
        for parameter in &mut parameters {
            // An unannotated / rest parameter carries no slot (`ty: None`) —
            // nothing to re-point.
            if let Some(ty) = parameter.ty.as_mut() {
                repoint(ty);
            }
        }
        signature.parameters = parameters.into();
    }
    true
}

#[cfg(test)]
mod checked_ordinal_tests {
    use super::checked_rebased_ordinal;

    #[test]
    fn signature_rebase_rejects_overflow_instead_of_clamping() {
        assert_eq!(checked_rebased_ordinal(u32::MAX, 0), Some(u32::MAX));
        assert_eq!(checked_rebased_ordinal(u32::MAX, 1), None);
        if usize::BITS > u32::BITS {
            assert_eq!(checked_rebased_ordinal(0, u32::MAX as usize + 1), None);
        }
    }
}
