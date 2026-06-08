//! The two-sided positive-allowlist admission gate for the TS7
//! `TypeExpr`-projection oracle (`docs/arch/u0-oracle-harness-design.md` §Q2).
//!
//! A `(row, query)` is admissible for a hover-lowered snapshot ONLY when EVERY
//! construct in BOTH its TS7 hover answer AND its real fixture SOURCE
//! declaration(s) is on the closed POSITIVE ALLOWLIST — `default-REJECT` is the
//! rule. The gate is checked BEFORE lowering (a post-lowering check is unsound:
//! OXC has already silently erased the lossy construct — the worked `IdBranded`
//! `unique symbol` miss in §Q2). This module is the pure, offline-testable
//! admission LOGIC + the source-side `RawSourceSurface` data model:
//!
//! - the hover side walks the parsed OXC `TSType` AST (`admit_hover_text` /
//!   `admit_hover_ast`), where the lossy constructs STILL EXIST before OXC's
//!   `filter_map` erases them;
//! - the source side walks the COMBINED `(RawSourceSurface raw facts, lowered
//!   body `TypeExpr`)` pair of every contributor (`admit_source_contributor`) —
//!   the raw facts catch the silently-erased constructs (`unique symbol`,
//!   computed/symbol keys, member visibility, accessors, `abstract` ctor,
//!   `const`/variance type-params, `this` type/param, `as const` provenance,
//!   the overload SET, optional/labelled tuple shape), the lowered body catches
//!   the non-erased rejectable `TypeExpr` variants (`Conditional` / `Mapped` /
//!   callable / `TemplateLiteral` / `Infer` / `KeyOf` / `IndexedAccess` /
//!   `TypeOf` / `RecursiveRef`);
//! - the strict-lowering drop-counter (`lower_with_drop_count`) is the
//!   belt-and-suspenders backstop: an admitted AST must lower with ZERO dropped
//!   members;
//! - the §Q2 backstop reject rules (truncation marker, `any`/`never`,
//!   `Unknown`) fold into the same gate;
//! - the two-sided combiner (`admit_query`) requires the hover clean AND every
//!   source contributor clean AND the drop count zero AND the backstops pass,
//!   and applies the mode-dependent shallow-expansion fence.
//!
//! The allowlist PREDICATE is mode-INDEPENDENT: it admits the
//! single-contributor result surfaces (primitive / object / property / index
//! signature / union / intersection) and REJECTs every deferred construct
//! (`Conditional`, `Mapped`, `KeyOf`, `Skeleton` shell, `Unknown`, …)
//! regardless of mode, so the predicate does not branch on the projection mode.
//! `Shallow`, `Navigate`, and `Expanded` are all admissible modes (the lifted
//! index-signature + modifier-utility rows are captured in `Expanded`). The ONE
//! mode-dependent rule — the shallow-expansion display fence (§Q2
//! `shallow_hover_expansion_rejected`) — lives in `admit_query`, which is where
//! `ProjectionModeKind` is consumed: it fires only in `Shallow` / `Navigate` (a
//! hover that expanded a source bare-`Ref` is a tsgo display artefact there), and
//! is correctly skipped in `Expanded`, where expansion is the intended surface.
//!
//! The LIVE producers of the surfaces this predicate consumes now exist: the
//! parse-time `RawSourceSurface` capture (in
//! `verter_compiler::utils::oxc::vue::raw_surface`) fills the raw facts off the
//! OXC parse tree, and [`super::source_walk::resolve_source_declarations`]
//! binds a `SourceLocator` through the shared resolver to the
//! `SourceWalkResult` of contributors this gate walks. The predicate's sub-fns
//! remain ALSO exercised here with synthetic contributors / parsed hover text
//! by the discriminating guards (the gate must reject the named construct
//! whether the surface arrives live or synthetic).
//!
//! What is still deferred (the tsgo GENERATION side, `#[cfg(feature =
//! "oracle-gen")]`): the tsgo LSP driver that produces the hover answers, the
//! vendored env corpus, and the probe synthesizer — never part of the default
//! resolver build or test gate, preserving the `tsgo`-forbidden-at-runtime
//! invariant.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Statement, TSSignature, TSTupleElement, TSType, TSTypeName, TSTypeOperatorOperator,
};
use oxc_parser::Parser;
use oxc_span::SourceType;

use verter_type_expr::{MemberVisibility, ObjectMember, PrimitiveName, TypeExpr};

// The source-side raw-fact data model is the PRODUCTION parse-time capture
// (in `verter_parser`, re-exported through `verter_compiler`).
// There is ONE `RawSourceSurface` type: the admission predicate here consumes
// the exact records the parse pass stores on the content-addressed artifact, so
// the live `resolve_source_declarations` path feeds this gate without a second
// type. The admission VERDICT model (`SourceContributor` / `SourceWalkResult` /
// `AdmissionVerdict` / `RejectReason`) is admission-specific and stays here.
use verter_compiler::utils::oxc::vue::raw_surface::{
    RawKey, RawMemberKind, RawSourceSurface, TupleElementShape,
};

use super::normalize::ProjectionModeKind;

// ===========================================================================
// Source-side admission verdict model (admission-specific — the raw-fact DATA
// model `RawSourceSurface` / `RawKey` / … is the production parse-time capture
// re-exported above, NOT redefined here).
// ===========================================================================

/// One bound defining declaration the source-side walk resolved, pairing the
/// retained raw-fact record with the contributor's already-lowered body
/// `TypeExpr` (the shallow artifact the resolver already holds — NOT a
/// re-derived view).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceContributor {
    /// 0-based source/binder position in the merge group.
    pub(crate) ordinal: u16,
    /// The retained parse-time raw-fact record (erased facts).
    pub(crate) raw_surface: RawSourceSurface,
    /// The already-lowered `ShallowTypeSymbol.body` / `TypeDeclInfo.body` — the
    /// non-erased rejectable variants.
    pub(crate) lowered_body: TypeExpr,
}

/// The result of resolving a `SourceLocator` to its defining contributor(s).
/// `Unresolved` and `Cycle` both REJECT the `(row, query)` — the generator never
/// admits a capture whose real source it could not reach and walk.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SourceWalkResult {
    /// An ORDERED, ≥1 defining-decl contributor vector.
    Resolved {
        contributors: Vec<SourceContributor>,
    },
    /// The locator did not bind in the controlled fixture set.
    Unresolved,
    /// A visited-set re-entry on the transitive walk.
    Cycle,
}

// ===========================================================================
// Admission verdict
// ===========================================================================

/// The verdict of an admission walk. `default-REJECT` is the rule: any construct
/// not on the closed positive allowlist rejects.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AdmissionVerdict {
    /// Every construct is on the positive allowlist (and every predicate held).
    Admit,
    /// A non-allowlisted construct (or a backstop trigger) rejected the capture.
    Reject(RejectReason),
}

impl AdmissionVerdict {
    fn is_admit(&self) -> bool {
        matches!(self, AdmissionVerdict::Admit)
    }
}

/// WHY a capture was rejected. A closed, discriminating set — each variant names
/// the specific §Q2 REJECT construct or backstop, so a guard can assert the
/// EXACT reason, not merely "rejected".
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RejectReason {
    /// A construct absent from the closed positive ADMIT list (the default).
    NotOnAllowlist(&'static str),
    /// `unique symbol` type operator (`oxc/lib.rs:171`).
    UniqueSymbol,
    /// Computed / `symbol` / `unique symbol` object key (`oxc/lib.rs:99,921`).
    NonStaticKey,
    /// `this` type or `this` parameter (`oxc/lib.rs:223`, `lib.rs:927`).
    ThisTypeOrParam,
    /// `const` / variance (`in`/`out`) type parameter (`lib.rs:1018`).
    TypeParamModifier,
    /// `abstract` constructor type (`lib.rs:159`, `oxc/lib.rs:126`).
    AbstractCtor,
    /// `private` / `protected` member visibility (`oxc/lib.rs:427`).
    NonPublicVisibility,
    /// Getter / setter accessor (not an `ObjectMember` variant — `lib.rs:426`).
    Accessor,
    /// An overload SET / callable union/intersection arm / callable member.
    Callable,
    /// An enum-member / qualified-name `Ref` (`Color.Red`) — a nominal brand
    /// `TypeExpr` cannot carry.
    EnumMemberOrQualified,
    /// Optional / labelled / `| undefined` tuple element.
    TupleElementShape,
    /// A deferred construct (`Conditional` / `Mapped` / `TemplateLiteral` /
    /// `Infer` / `KeyOf` / `IndexedAccess` / `TypeOf`) outside a spike-proven
    /// mode.
    DeferredConstruct(&'static str),
    /// `RecursiveRef` — a self-referential surface cannot be a finite hover.
    RecursiveRef,
    /// A `value`/`typeof` referent carrying an `as const` provenance fact.
    ConstAssertion,
    /// `any` in a concrete-type position (backstop 3).
    AnyKeyword,
    /// `never` outside a genuine closed empty union (backstop 3).
    NeverKeyword,
    /// Parsing the hover left a `TypeExpr::Unknown` / parse leftovers
    /// (backstop 2).
    UnknownOrParseLeftover,
    /// A truncation / ellipsis marker in the raw hover text (backstop 1).
    TruncationMarker,
    /// The hover RHS did not parse to a single type.
    HoverUnparsable,
    /// The source-side locator did not bind, or a cyclic source surface.
    SourceUnresolvedOrCyclic,
    /// A strict-lowering drop count > 0 (the AST lost a member/param OXC
    /// `filter_map`-dropped).
    StrictLoweringDrop(usize),
    /// In a shallow / navigate mode the hover EXPANDED a userland alias the
    /// source kept as a bare `Ref` (tsgo display artefact — §Q2
    /// `shallow_hover_expansion_rejected`).
    ShallowHoverExpansion,
}

// ===========================================================================
// Source-side admission (the COMBINED raw-fact + lowered-body walk)
// ===========================================================================

/// Walk ONE contributor's COMBINED `(raw facts, lowered body)` pair against the
/// positive allowlist. Admission requires BOTH halves clean.
#[allow(dead_code)]
pub(crate) fn admit_source_contributor(contributor: &SourceContributor) -> AdmissionVerdict {
    let raw = admit_raw_surface(&contributor.raw_surface);
    if !raw.is_admit() {
        return raw;
    }
    admit_lowered_body(&contributor.lowered_body)
}

/// Walk every contributor of a resolved source walk; admission requires ALL
/// clean. `Unresolved` / `Cycle` reject.
#[allow(dead_code)]
pub(crate) fn admit_source_walk(walk: &SourceWalkResult) -> AdmissionVerdict {
    match walk {
        SourceWalkResult::Unresolved | SourceWalkResult::Cycle => {
            AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic)
        }
        SourceWalkResult::Resolved { contributors } => {
            if contributors.is_empty() {
                return AdmissionVerdict::Reject(RejectReason::SourceUnresolvedOrCyclic);
            }
            for c in contributors {
                let v = admit_source_contributor(c);
                if !v.is_admit() {
                    return v;
                }
            }
            AdmissionVerdict::Admit
        }
    }
}

/// The RAW-fact half of source admission — catches the SILENTLY-ERASED
/// constructs the lowered body lost.
#[allow(dead_code)]
pub(crate) fn admit_raw_surface(raw: &RawSourceSurface) -> AdmissionVerdict {
    if !raw.unique_symbol_ops.is_empty() {
        return AdmissionVerdict::Reject(RejectReason::UniqueSymbol);
    }
    if raw
        .raw_member_keys
        .iter()
        .any(|k| !matches!(k, RawKey::Static(_)))
    {
        return AdmissionVerdict::Reject(RejectReason::NonStaticKey);
    }
    if raw
        .member_kinds
        .iter()
        .any(|k| matches!(k, RawMemberKind::Getter | RawMemberKind::Setter))
    {
        return AdmissionVerdict::Reject(RejectReason::Accessor);
    }
    if raw.member_kinds.iter().any(|k| {
        matches!(
            k,
            RawMemberKind::Method
                | RawMemberKind::CallSignature
                | RawMemberKind::ConstructSignature
        )
    }) {
        return AdmissionVerdict::Reject(RejectReason::Callable);
    }
    if raw
        .member_visibility
        .iter()
        .any(|v| !matches!(v, MemberVisibility::Public))
    {
        return AdmissionVerdict::Reject(RejectReason::NonPublicVisibility);
    }
    if raw.abstract_ctor {
        return AdmissionVerdict::Reject(RejectReason::AbstractCtor);
    }
    if raw.type_param_modifiers.iter().any(|m| m.is_present()) {
        return AdmissionVerdict::Reject(RejectReason::TypeParamModifier);
    }
    if raw.this_type_or_param {
        return AdmissionVerdict::Reject(RejectReason::ThisTypeOrParam);
    }
    if raw.value_const_assertion == Some(true) {
        return AdmissionVerdict::Reject(RejectReason::ConstAssertion);
    }
    if raw.overload_signatures.len() >= 2 {
        return AdmissionVerdict::Reject(RejectReason::Callable);
    }
    if raw
        .tuple_element_shape
        .iter()
        .any(|s| !matches!(s, TupleElementShape::Plain))
    {
        return AdmissionVerdict::Reject(RejectReason::TupleElementShape);
    }
    AdmissionVerdict::Admit
}

/// The lowered-body half of source admission — catches the NON-erased rejectable
/// `TypeExpr` variants AND the `any`/`never`/`Unknown` backstops. Shares the
/// recursive `TypeExpr` predicate with the oracle-value backstop.
#[allow(dead_code)]
pub(crate) fn admit_lowered_body(body: &TypeExpr) -> AdmissionVerdict {
    admit_type_expr(body)
}

/// The recursive positive-allowlist predicate over a `TypeExpr` (the lowered
/// source body OR the lowered oracle value). `default-REJECT`.
#[allow(dead_code)]
pub(crate) fn admit_type_expr(expr: &TypeExpr) -> AdmissionVerdict {
    match expr {
        TypeExpr::Primitive(p) => match p {
            PrimitiveName::Any => AdmissionVerdict::Reject(RejectReason::AnyKeyword),
            // A lone `never` is rejected (backstop 3). `never` as the *only*
            // arm of an empty union does not reach here as a bare `Primitive`.
            PrimitiveName::Never => AdmissionVerdict::Reject(RejectReason::NeverKeyword),
            _ => AdmissionVerdict::Admit,
        },
        TypeExpr::Literal(_) => AdmissionVerdict::Admit,
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
            for arm in arms.iter() {
                // A callable arm is an overload group / function arm — REJECT.
                if is_callable_type_expr(arm) {
                    return AdmissionVerdict::Reject(RejectReason::Callable);
                }
                let v = admit_type_expr(arm);
                if !v.is_admit() {
                    return v;
                }
            }
            AdmissionVerdict::Admit
        }
        TypeExpr::Array { element, .. } => admit_type_expr(element),
        TypeExpr::Tuple { elements, .. } => {
            for el in elements.iter() {
                let v = admit_type_expr(&el.ty);
                if !v.is_admit() {
                    return v;
                }
            }
            AdmissionVerdict::Admit
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(p) => {
                        if !matches!(p.visibility, MemberVisibility::Public) {
                            return AdmissionVerdict::Reject(RejectReason::NonPublicVisibility);
                        }
                        let v = admit_type_expr(&p.ty);
                        if !v.is_admit() {
                            return v;
                        }
                    }
                    ObjectMember::IndexSignature(idx) => {
                        let v = admit_type_expr(&idx.value_type);
                        if !v.is_admit() {
                            return v;
                        }
                    }
                    ObjectMember::CallSignature(_)
                    | ObjectMember::ConstructSignature(_)
                    | ObjectMember::Method(_) => {
                        return AdmissionVerdict::Reject(RejectReason::Callable);
                    }
                }
            }
            AdmissionVerdict::Admit
        }
        // A plain userland / package / builtin `Ref` is the correct shallow
        // surface (an enum-member / qualified-name ref is caught hover-side as a
        // qualified name; a lowered `Ref` carries no qualification).
        TypeExpr::Ref { type_arguments, .. } => {
            for arg in type_arguments.iter() {
                let v = admit_type_expr(arg);
                if !v.is_admit() {
                    return v;
                }
            }
            AdmissionVerdict::Admit
        }
        // Skeleton shells are only constructed in the deferred Skeleton mode; a
        // bare `TypeParameter` is structurally harmless in shallow/navigate.
        TypeExpr::TypeParameter(_) => AdmissionVerdict::Admit,
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => admit_type_expr(inner),
        // Non-erased rejectable variants — deferred result constructs, rejected
        // in every currently-admissible mode.
        TypeExpr::KeyOf(_) => AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof")),
        TypeExpr::TypeOf(_) => AdmissionVerdict::Reject(RejectReason::DeferredConstruct("typeof")),
        TypeExpr::IndexedAccess { .. } => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
        }
        TypeExpr::Conditional { .. } => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("conditional"))
        }
        TypeExpr::Mapped { .. } => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("mapped"))
        }
        TypeExpr::TemplateLiteral { .. } => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("template-literal"))
        }
        TypeExpr::Infer { .. } => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("infer"))
        }
        TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => {
            AdmissionVerdict::Reject(RejectReason::Callable)
        }
        TypeExpr::RecursiveRef { .. } => AdmissionVerdict::Reject(RejectReason::RecursiveRef),
        TypeExpr::SyntheticSlotBinding(_) => {
            AdmissionVerdict::Reject(RejectReason::NotOnAllowlist("synthetic-slot-binding"))
        }
        TypeExpr::Unknown { .. } => AdmissionVerdict::Reject(RejectReason::UnknownOrParseLeftover),
    }
}

/// Whether a `TypeExpr` is callable (a function, a constructor type, or an
/// object whose only/any members are call/construct/method signatures) — a
/// callable arm/member is an overload-group surface (REJECT).
fn is_callable_type_expr(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Function(_) | TypeExpr::ConstructorType(_) => true,
        TypeExpr::Object(obj) => obj.properties.iter().any(|m| {
            matches!(
                m,
                ObjectMember::CallSignature(_)
                    | ObjectMember::ConstructSignature(_)
                    | ObjectMember::Method(_)
            )
        }),
        _ => false,
    }
}

// ===========================================================================
// Hover-side admission (walk the parsed OXC TSType BEFORE lowering)
// ===========================================================================

/// Parse a hover RHS type-text and run the full hover-side admission: the §Q2
/// backstop reject rules (truncation marker), the positive-allowlist walk over
/// the raw OXC AST, and the strict-lowering drop-counter. Returns the verdict;
/// `admit_query` combines it with the source side.
#[allow(dead_code)]
pub(crate) fn admit_hover_text(rhs: &str) -> AdmissionVerdict {
    if hover_text_truncated(rhs) {
        return AdmissionVerdict::Reject(RejectReason::TruncationMarker);
    }
    let allocator = Allocator::default();
    let wrapped = format!("type __oracle_probe__ = {rhs};");
    let ret = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();
    if ret.panicked {
        return AdmissionVerdict::Reject(RejectReason::HoverUnparsable);
    }
    let alias = ret.program.body.iter().find_map(|stmt| match stmt {
        Statement::TSTypeAliasDeclaration(alias) if alias.id.name == "__oracle_probe__" => {
            Some(&alias.type_annotation)
        }
        _ => None,
    });
    let Some(ts_type) = alias else {
        return AdmissionVerdict::Reject(RejectReason::HoverUnparsable);
    };
    let hover_verdict = admit_hover_ast(ts_type);
    if !hover_verdict.is_admit() {
        return hover_verdict;
    }
    // Belt-and-suspenders: an admitted AST must lower with ZERO drops.
    let (lowered, drops) = lower_with_drop_count(ts_type, &wrapped);
    if drops > 0 {
        return AdmissionVerdict::Reject(RejectReason::StrictLoweringDrop(drops));
    }
    // The lowered value must also clear the `TypeExpr` backstop (catches an
    // `Unknown` / `any` / `never` the AST walk did not name).
    admit_type_expr(&lowered)
}

/// Lower a hover RHS type-text to a `TypeExpr` through the SAME OXC parse +
/// `lower_ts_type` the generator uses (reusing this module's parse so the spike
/// / generator do not duplicate the OXC wiring). Returns `None` on a parse
/// failure or a non-zero strict-lowering drop count. The admission verdict is a
/// SEPARATE concern — call `admit_hover_text` for that; this is only the value.
#[allow(dead_code)]
pub(crate) fn lower_hover_rhs(rhs: &str) -> Option<TypeExpr> {
    let allocator = Allocator::default();
    let wrapped = format!("type __oracle_probe__ = {rhs};");
    let ret = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();
    if ret.panicked {
        return None;
    }
    let ts_type = ret.program.body.iter().find_map(|stmt| match stmt {
        Statement::TSTypeAliasDeclaration(alias) if alias.id.name == "__oracle_probe__" => {
            Some(&alias.type_annotation)
        }
        _ => None,
    })?;
    let (lowered, drops) = lower_with_drop_count(ts_type, &wrapped);
    if drops > 0 {
        return None;
    }
    Some(lowered)
}

/// The positive-allowlist walk over the RAW OXC `TSType` AST — `default-REJECT`.
/// The lossy constructs STILL EXIST here (before OXC's `filter_map` erases
/// them), so this is where `unique symbol`, computed keys, `this` types,
/// accessors, conditionals, callables, and qualified-name refs are caught.
#[allow(dead_code)]
pub(crate) fn admit_hover_ast(ts: &TSType<'_>) -> AdmissionVerdict {
    match ts {
        TSType::TSStringKeyword(_)
        | TSType::TSNumberKeyword(_)
        | TSType::TSBooleanKeyword(_)
        | TSType::TSBigIntKeyword(_)
        | TSType::TSSymbolKeyword(_)
        | TSType::TSVoidKeyword(_)
        | TSType::TSNullKeyword(_)
        | TSType::TSUndefinedKeyword(_)
        | TSType::TSObjectKeyword(_)
        | TSType::TSUnknownKeyword(_) => AdmissionVerdict::Admit,
        TSType::TSAnyKeyword(_) => AdmissionVerdict::Reject(RejectReason::AnyKeyword),
        TSType::TSNeverKeyword(_) => AdmissionVerdict::Reject(RejectReason::NeverKeyword),
        TSType::TSLiteralType(_) => AdmissionVerdict::Admit,
        TSType::TSArrayType(arr) => admit_hover_ast(&arr.element_type),
        TSType::TSParenthesizedType(p) => admit_hover_ast(&p.type_annotation),
        TSType::TSTupleType(tuple) => {
            for el in &tuple.element_types {
                match el {
                    TSTupleElement::TSOptionalType(_)
                    | TSTupleElement::TSNamedTupleMember(_)
                    | TSTupleElement::TSRestType(_) => {
                        return AdmissionVerdict::Reject(RejectReason::TupleElementShape);
                    }
                    other => {
                        // A plain element is itself a `TSType`.
                        if let Some(inner) = other.as_ts_type() {
                            let v = admit_hover_ast(inner);
                            if !v.is_admit() {
                                return v;
                            }
                        } else {
                            return AdmissionVerdict::Reject(RejectReason::TupleElementShape);
                        }
                    }
                }
            }
            AdmissionVerdict::Admit
        }
        TSType::TSUnionType(u) => {
            for arm in &u.types {
                let v = admit_hover_ast(arm);
                if !v.is_admit() {
                    return v;
                }
            }
            AdmissionVerdict::Admit
        }
        TSType::TSIntersectionType(i) => {
            for arm in &i.types {
                let v = admit_hover_ast(arm);
                if !v.is_admit() {
                    return v;
                }
            }
            AdmissionVerdict::Admit
        }
        TSType::TSTypeLiteral(lit) => {
            for member in &lit.members {
                let v = admit_hover_signature(member);
                if !v.is_admit() {
                    return v;
                }
            }
            AdmissionVerdict::Admit
        }
        TSType::TSTypeReference(r) => {
            // A qualified name (`Color.Red`) is an enum-member / namespace ref —
            // its nominal brand `TypeExpr` cannot carry, so REJECT.
            match &r.type_name {
                TSTypeName::QualifiedName(_) => {
                    return AdmissionVerdict::Reject(RejectReason::EnumMemberOrQualified);
                }
                TSTypeName::ThisExpression(_) => {
                    return AdmissionVerdict::Reject(RejectReason::ThisTypeOrParam);
                }
                TSTypeName::IdentifierReference(_) => {}
            }
            if let Some(args) = &r.type_arguments {
                for arg in &args.params {
                    let v = admit_hover_ast(arg);
                    if !v.is_admit() {
                        return v;
                    }
                }
            }
            AdmissionVerdict::Admit
        }
        TSType::TSTypeOperatorType(op) => match op.operator {
            TSTypeOperatorOperator::Unique => AdmissionVerdict::Reject(RejectReason::UniqueSymbol),
            TSTypeOperatorOperator::Keyof => {
                AdmissionVerdict::Reject(RejectReason::DeferredConstruct("keyof"))
            }
            // `readonly T[]` / `readonly [A, B]` preserves readonly losslessly.
            TSTypeOperatorOperator::Readonly => admit_hover_ast(&op.type_annotation),
        },
        TSType::TSIndexedAccessType(_) => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("indexed-access"))
        }
        TSType::TSTypeQuery(_) => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("typeof"))
        }
        TSType::TSConditionalType(_) => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("conditional"))
        }
        TSType::TSMappedType(_) => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("mapped"))
        }
        TSType::TSTemplateLiteralType(_) => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("template-literal"))
        }
        TSType::TSInferType(_) => {
            AdmissionVerdict::Reject(RejectReason::DeferredConstruct("infer"))
        }
        TSType::TSFunctionType(_) => AdmissionVerdict::Reject(RejectReason::Callable),
        TSType::TSConstructorType(_) => AdmissionVerdict::Reject(RejectReason::Callable),
        TSType::TSThisType(_) => AdmissionVerdict::Reject(RejectReason::ThisTypeOrParam),
        // default-REJECT: any node not enumerated above falls through here.
        _ => AdmissionVerdict::Reject(RejectReason::NotOnAllowlist("unenumerated-ts-node")),
    }
}

/// Admit a single object/interface type-literal member on the hover side.
fn admit_hover_signature(sig: &TSSignature<'_>) -> AdmissionVerdict {
    match sig {
        TSSignature::TSPropertySignature(prop) => {
            if prop.computed {
                return AdmissionVerdict::Reject(RejectReason::NonStaticKey);
            }
            match &prop.type_annotation {
                Some(ann) => admit_hover_ast(&ann.type_annotation),
                // A property with no annotation lowers to `any` — REJECT.
                None => AdmissionVerdict::Reject(RejectReason::AnyKeyword),
            }
        }
        TSSignature::TSIndexSignature(idx) => admit_hover_ast(&idx.type_annotation.type_annotation),
        TSSignature::TSMethodSignature(_) => AdmissionVerdict::Reject(RejectReason::Callable),
        TSSignature::TSCallSignatureDeclaration(_) => {
            AdmissionVerdict::Reject(RejectReason::Callable)
        }
        TSSignature::TSConstructSignatureDeclaration(_) => {
            AdmissionVerdict::Reject(RejectReason::Callable)
        }
    }
}

// ===========================================================================
// Strict-lowering drop-counter (§Q2 step 3)
// ===========================================================================

/// Lower an OXC `TSType` AND count the members OXC's lowering `filter_map`-drops
/// (`oxc/lib.rs:99` — a member whose key `property_key_name` returns `None`,
/// i.e. a computed / `symbol` / `unique symbol` key, `oxc/lib.rs:921`). On an
/// allowlist-clean AST the count is ZERO; a non-zero count means the allowlist
/// admitted something it should not have. Returns `(lowered, drop_count)`.
#[allow(dead_code)]
pub(crate) fn lower_with_drop_count(ts: &TSType<'_>, source: &str) -> (TypeExpr, usize) {
    let drops = count_droppable_members(ts);
    let lowered = verter_type_expr_oxc::lower_ts_type(ts, source);
    (lowered, drops)
}

/// Count, recursively over the AST, the type-literal members whose key is
/// non-static (computed) — exactly the members OXC silently elides.
fn count_droppable_members(ts: &TSType<'_>) -> usize {
    let mut count = 0;
    match ts {
        TSType::TSTypeLiteral(lit) => {
            for member in &lit.members {
                match member {
                    TSSignature::TSPropertySignature(prop) => {
                        if prop.computed {
                            count += 1;
                        } else if let Some(ann) = &prop.type_annotation {
                            count += count_droppable_members(&ann.type_annotation);
                        }
                    }
                    TSSignature::TSMethodSignature(m) => {
                        if m.computed {
                            count += 1;
                        }
                    }
                    TSSignature::TSIndexSignature(idx) => {
                        count += count_droppable_members(&idx.type_annotation.type_annotation);
                    }
                    _ => {}
                }
            }
        }
        TSType::TSArrayType(arr) => count += count_droppable_members(&arr.element_type),
        TSType::TSParenthesizedType(p) => count += count_droppable_members(&p.type_annotation),
        TSType::TSUnionType(u) => {
            for arm in &u.types {
                count += count_droppable_members(arm);
            }
        }
        TSType::TSIntersectionType(i) => {
            for arm in &i.types {
                count += count_droppable_members(arm);
            }
        }
        TSType::TSTupleType(tuple) => {
            for el in &tuple.element_types {
                if let Some(inner) = el.as_ts_type() {
                    count += count_droppable_members(inner);
                }
            }
        }
        TSType::TSTypeOperatorType(op) => count += count_droppable_members(&op.type_annotation),
        _ => {}
    }
    count
}

// ===========================================================================
// Backstop reject rules + the two-sided combiner
// ===========================================================================

/// Backstop rule 1: a `…` / `...` truncation marker in the raw hover text means
/// TS truncated a large type — checked BEFORE parsing. The unicode `…` (U+2026)
/// is unambiguous. An ASCII `...` is treated as truncation ONLY when it is a
/// DANGLING ellipsis (followed by whitespace / a closer / end-of-text), NOT when
/// it is a `...T` rest / spread token (followed by an identifier / `[` / `(` /
/// `{` / `.`) — otherwise a legitimate rest tuple `[...T[]]` would false-trip.
#[allow(dead_code)]
pub(crate) fn hover_text_truncated(hover: &str) -> bool {
    if hover.contains('\u{2026}') {
        return true;
    }
    let mut search_from = 0;
    while let Some(rel) = hover[search_from..].find("...") {
        let after = search_from + rel + 3;
        let next = hover[after..].chars().next();
        let is_rest_or_spread = matches!(
            next,
            Some(c) if c.is_alphanumeric() || c == '_' || c == '[' || c == '(' || c == '{' || c == '.'
        );
        if !is_rest_or_spread {
            return true;
        }
        search_from = after;
    }
    false
}

/// The TWO-SIDED admission combiner (§Q2): a `(row, query)` is admissible ONLY
/// when the hover capture AND every resolved source contributor walk the
/// positive allowlist clean, the strict-lowering drop count is zero, and the
/// backstops pass. Additionally, in a shallow / navigate mode a hover that
/// EXPANDED a userland alias the source kept as a bare `Ref` is rejected as a
/// tsgo display artefact (`shallow_hover_expansion_rejected`).
#[allow(dead_code)]
pub(crate) fn admit_query(
    hover_rhs: &str,
    source_walk: &SourceWalkResult,
    mode: ProjectionModeKind,
) -> AdmissionVerdict {
    let source = admit_source_walk(source_walk);
    if !source.is_admit() {
        return source;
    }
    let hover = admit_hover_text(hover_rhs);
    if !hover.is_admit() {
        return hover;
    }
    if matches!(
        mode,
        ProjectionModeKind::Shallow | ProjectionModeKind::Navigate
    ) && shallow_hover_expanded_a_source_ref(hover_rhs, source_walk)
    {
        return AdmissionVerdict::Reject(RejectReason::ShallowHoverExpansion);
    }
    AdmissionVerdict::Admit
}

/// Whether, in a shallow mode, the SOURCE kept the queried symbol as a bare
/// userland `Ref` while the HOVER printed an expanded object/union instead of
/// the alias NAME — the §Q2 shallow-expansion display artefact.
fn shallow_hover_expanded_a_source_ref(hover_rhs: &str, source_walk: &SourceWalkResult) -> bool {
    let SourceWalkResult::Resolved { contributors } = source_walk else {
        return false;
    };
    // Source side is a single bare userland `Ref` …
    let source_is_bare_ref = contributors.len() == 1
        && matches!(
            &contributors[0].lowered_body,
            TypeExpr::Ref { type_arguments, .. } if type_arguments.is_empty()
        );
    if !source_is_bare_ref {
        return false;
    }
    // … but the hover printed a STRUCTURAL surface (an object/union/intersection
    // body) rather than re-printing the alias name.
    let trimmed = hover_rhs.trim_start();
    trimmed.starts_with('{')
        || (trimmed.contains('|') && !trimmed.starts_with('|'))
        || trimmed.contains('&')
}

#[cfg(test)]
mod tests;
