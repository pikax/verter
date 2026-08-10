//! Recursive flow-return EXPECTATIONS, the public cold/warm boundary
//! companion, and the crossed capture-write position matrix — the U6
//! corpus-harness strengthening layer.
//!
//! # Why this module exists
//!
//! The root-`NodeShape` corpus assertion cannot distinguish the VALUES a
//! row's own `checker` column names: `() => "a"` and `() => "b"` are both
//! `NodeShape::Other`; `"a" | "b"` and `"a" | undefined` are both
//! `NodeShape::Union`. Five rows shipped that way and characterized
//! nothing. This module adds:
//!
//! 1. **A recursive [`ExpectedNode`] expectation** able to assert
//!    signatures, exact literal and primitive values, intersections, and
//!    order-insensitive but EXACT union constituent sets (set equality,
//!    never subset), to arbitrary member depth.
//! 2. **Graph-level identity, preserved.** The `TypeExpr` raise boundary
//!    projects `TypeParam` / `DeclRef` / `BareRef` all to `Ref { name }`
//!    (see `SemanticNodeData::DeclRef` / `BareRef` docs), and the corpus's
//!    own `NodeShape` buckets all three as `Other` — two independent
//!    conflation sites. [`ExpectedNode`] carries a DISTINCT variant for
//!    each, matched on the graph node itself, so an expectation can (and
//!    the controls do) reject one where another is measured.
//! 3. **A public-boundary companion** ([`Boundary::Audit`] /
//!    [`Boundary::AuditRefusal`]) through
//!    `VerterHost::get_flow_return_type_with_audit`, invoked TWICE per
//!    row, with BOTH calls modelled explicitly (result class, typed
//!    degradation, projected JSON, `from_cache`, cold-compute count):
//!    the first call must be COLD (`from_cache == false`,
//!    `cold_computes >= 1`) with the pinned typed `degradation` and the
//!    EXACT projected JSON (`project_node_to_type_expr_json_bytes`); the
//!    second call must keep the first call's result CLASS and typed
//!    degradation, and its cache-replay state is pinned (`warm_replay`) —
//!    a clean result must replay warm with ZERO cold computes and an
//!    identical projection, and a degraded (`ReturnOnly`) result must NOT
//!    be admitted warm AND must genuinely COLD-COMPUTE again
//!    (`from_cache == false` alone is not cold replay). A refusal row
//!    pins [`Boundary::AuditRefusal`]: both calls refuse, and the second
//!    refusal is never served warm and always recomputes — the typed
//!    non-admission contract. The assertions fail in both directions, so
//!    a result admitted warm when it must stay cold is a failure, not a
//!    speedup.
//! 4. **The crossed capture-write matrix** ([`matrix`]): binding kind ×
//!    write timing × closure depth × expression position × guard kind ×
//!    completion container, with the KEY assertion that the same
//!    capture-write effect surfaces for a given cell REGARDLESS of
//!    expression position — the assertion that makes a position-specific
//!    effect hook impossible to reintroduce silently.
//!
//! # Oracle / profile stamps
//!
//! Every assertion this module makes is stamped: [`ORACLE_STAMP`] names
//! the pinned checker every `checker` value was measured against, and
//! [`PROFILE_STAMP`] names the exact semantic profile in force for every
//! measurement. Both stamps ride every failure message.
//!
//! # Expected-versus-actual gap rows
//!
//! This module does NOT fix flow semantics. A strengthened row whose deep
//! measurement DIVERGES from its `checker` column is pinned to the ACTUAL
//! measured value with `Verdict::KnownOwed` — a visible, recorded
//! expected-versus-actual gap that fails the moment the owning block
//! repairs the semantics (forcing a deliberate re-pin), and fails if the
//! shape degrades further.
//!
//! # Negative controls — the comparators themselves are characterized
//!
//! The controls in [`expectation_controls`] and [`matrix_outcome_controls`]
//! prove every comparison clause this module RETAINS can fail, so
//! neutering any retained comparison clause leaves a failing control,
//! never a silently green suite. Comparison vocabulary that neither a
//! corpus row nor a control exercises is DELETED rather than shipped
//! uncharacterizable — but ONLY under an exhaustive reachability
//! argument over the type-expression grammar, never a handful of sample
//! probes. Deleting the `SignatureKind::Call` discriminant violates that
//! standard and is REVERSED here: three sample probes (`return class C
//! {}` → `any`; a `{ new (): Box }`-typed value → `{  }`; an alias to
//! `new () => Box` → `DeclRef`) were read as "no `Construct` signature
//! is reachable on this rail", but the annotation-typed parameter form
//! (`function makeProps(x: new () => Box) { return x }`) DOES reach a
//! `SignatureKind::Construct` node, so the deletion had made every
//! signature pin accept a construct signature where a call signature
//! was pinned. The discriminant is RESTORED in both comparators
//! ([`node_matches`] and [`checker_syntax::matches_node`]) and
//! controlled live in both directions by
//! [`expectation_controls::construct_signature_is_distinct_from_call_signature`].
//!
//! The surviving deletions, each with its DIRECTIONAL exhaustive
//! argument (an accept-arm removal narrows the matcher — every deleted
//! pair now reaches the controlled `_ => false` catchall — so unlike a
//! rejection-clause removal it can never widen what a pin accepts):
//!
//! * `Lit::Bool` / `Lit::BigInt` — EXPECTED-side vocabulary only.
//!   [`lit_matches`] is total over `(Lit, LiteralValue)` via its
//!   catchall: the retained `Str`/`Num` arms match only their own
//!   measured kinds, so a measured boolean/bigint literal is REJECTED
//!   by every retained pin (fail-closed), and no row's `checker`
//!   column names a boolean/bigint literal (the checker-syntax parser
//!   rejects `true`/`false` prints loudly and models no bigint print).
//!   Re-add a variant together with its control when a row needs it;
//! * alias transparency in the matcher — a TRANSPARENCY removal, which
//!   STRICTENS: no `node_matches` / `checker_syntax::matches_node` arm
//!   carries `Alias` on the measured side, so every `Alias` node
//!   reaches the controlled `_ => false` for EVERY expected form
//!   (exhaustive over the match arms, not sampled) and the failure
//!   report renders it as `Alias(…)` — fail-closed, loud. A future
//!   alias-producing row must add transparency together with a control
//!   that discriminates it.
//!
//! Covered, precisely:
//!
//! * every [`ExpectedNode`] form — exact literal STRING and NUMBER
//!   values, literal-versus-primitive widening, signature KIND (a call
//!   pin rejects a live construct signature and vice versa), signature
//!   parameter ARITY, ordered parameter types, and return types, union
//!   set equality (subset / superset / swapped / duplicate), intersection
//!   arms (wrong name, missing arm, REVERSED source order, reference
//!   identity), the `TypeParam` / `DeclRef` / `BareRef` trio at VARIANT
//!   level and at NAME level, object members (wrong value, wrong name,
//!   missing, extra, duplicate-key injectivity), and the typed
//!   unmodelled-position marker;
//! * the fail-closed STRUCTURAL clauses of BOTH matchers — the depth
//!   guard (an exhausted depth never reads as a match) and the
//!   absent-node early-out (an evicted/unknown node id never reads as a
//!   match), each exercised directly by
//!   [`expectation_controls::depth_and_absent_node_guards_fail_closed`],
//!   plus each matcher's `_ => false` catchall (cross-variant pairs are
//!   rejected, held by the cross-class control assertions);
//! * every [`check_boundary`] clause — the no-value carrier, both
//!   first-call-cold clauses, typed degradation, exact projected JSON,
//!   result-class drift across the replay, typed-degradation drift
//!   across the replay, both warm-replay clauses INDIVIDUALLY, both
//!   cold-replay clauses INDIVIDUALLY (warm non-admission AND the second
//!   call actually cold-computing), and replay-projection drift;
//! * every [`check_boundary_refusal`] clause — the value-versus-refusal
//!   class in both directions, the cold first call, the PINNED typed
//!   refusal KIND of call 1, refusal stability on the second call,
//!   refusal-IDENTITY stability across the two calls (a refusal that
//!   changes kind between calls fails), warm non-admission of the
//!   refusal, and the second call's genuine cold recompute;
//! * every [`check_cell_outcome`] clause — the outcome class in both
//!   directions (a `NoValue` pin against a measured value, a `Value` pin
//!   against a measured refusal), the exact rendering, the typed
//!   degradation, BOTH first-call-cold clauses (shared with
//!   [`check_boundary`]), and the full second-call replay model (shared
//!   with [`check_boundary`] through one clause set, so a matrix cell
//!   and a corpus row cannot drift apart); a `NoValue` cell pins its
//!   typed refusal kind through the same [`check_boundary_refusal`]
//!   delegation the corpus refusal rows use;
//! * the structural clauses of [`checker_syntax::matches_node`] — union
//!   length and constituent injectivity, intersection length, object
//!   length and member injectivity, the function-print `Call`-kind
//!   discriminant, its depth guard, its absent-node early-out, and its
//!   `_ => false` catchall — held by
//!   [`expectation_controls::checker_syntax_structural_clauses_fail_closed`]
//!   together with the corpus-level checker-column mutation controls.
//!
//! Each control feeds a deliberately WRONG pin against a REAL
//! measurement. The boundary-clause controls that need a trace no fresh
//! host can produce (a warm first call; a single replay field in
//! isolation; a drifted replay; a class-drifted or cached refusal) build
//! it exclusively from measured values — a re-labelled or
//! single-field-substituted real trace, stated inline at each site.
//! `stamps_match_the_pinned_oracle_and_profile` is a consistency check
//! over the stamps, not a negative control, and is not counted as one.

use std::sync::Arc;

use super::{degr_of, upsert, Degr};
use crate::host_flow_return_audit::FlowReturnError;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowReturnFailure, FlowReturnUnsupported, LiteralValue, PrimitiveKind, QueryError,
    ReturnProjectionDemand, SemanticNodeData, SemanticNodeId, SignatureKind,
};
use crate::types::HostConfig;
use crate::{FileLanguage, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

// ─────────────────────────────────────────────────────────────────────────
// Oracle / profile stamps
// ─────────────────────────────────────────────────────────────────────────

/// The oracle every `checker` value in this module (and every matrix
/// cell's `checker` column) was measured against. CHECKER only, never
/// `.d.ts` emission.
pub(crate) const ORACLE_STAMP: &str =
    "tsgo 7.0.0-dev.20260526.1 --noEmit --strict --ignoreConfig --pretty false (checker only)";

/// The semantic profile in force for every measurement this module
/// takes: host construction, demand point, and rail.
pub(crate) const PROFILE_STAMP: &str = "VerterHost standalone { analysis_level: Full, \
     audit_enabled: true, footprint_capture: false, scheduler cpu_threads: 1 }; \
     demand = ReturnProjectionDemand::whole_return(); \
     rail = body-derived FlowReturn via VerterHost::get_flow_return_type_with_audit";

// ─────────────────────────────────────────────────────────────────────────
// The recursive expectation
// ─────────────────────────────────────────────────────────────────────────

/// An exact literal value expectation.
///
/// Deliberately NOT total over [`LiteralValue`]: only variants a corpus
/// row or a negative control exercises are kept, so every comparison
/// clause in [`lit_matches`] is characterized. `Bool` / `BigInt` were
/// DELETED (no row and no control measured either on this rail); re-add
/// a variant together with its control when a row needs it.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Lit {
    Str(&'static str),
    Num(f64),
}

/// A RECURSIVE graph-node expectation.
///
/// Matched against [`SemanticNodeData`] at the GRAPH-NODE level — never
/// against the projected `TypeExpr`, which conflates `TypeParam` /
/// `DeclRef` / `BareRef` into `Ref { name }`. Every variant must match
/// structurally, recursively, to arbitrary depth. `Alias` nodes match
/// NOTHING: the previously-claimed alias transparency was proven
/// unexercised (neutering the deref left the whole u6_flow suite green),
/// so it was deleted per the no-uncharacterized-comparison rule — an
/// `Alias` node fails loudly, rendered as `Alias(…)`, and a future
/// alias-producing row must reintroduce transparency WITH a control.
///
/// Like [`Lit`], the vocabulary is exercised, not speculative: every
/// variant is pinned by a corpus row or discriminated by a control.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ExpectedNode {
    /// `SemanticNodeData::Literal` with EXACTLY this value.
    Literal(Lit),
    /// `SemanticNodeData::Primitive` with EXACTLY this kind.
    Primitive(PrimitiveKind),
    /// `SemanticNodeData::Union` whose constituent set EQUALS this set —
    /// order-insensitive, but EXACT: a subset match and a superset match
    /// both FAIL. Duplicate expectations must be satisfied by distinct
    /// constituents.
    Union(&'static [ExpectedNode]),
    /// `SemanticNodeData::Intersection` with EXACTLY these arms, in
    /// source order.
    Intersection(&'static [ExpectedNode]),
    /// A `SemanticNodeData::Signature` with `SignatureKind::Call`,
    /// exactly these parameter types (ordered, exact arity), and this
    /// return type. This is what makes `() => "a"` distinguishable from
    /// `() => "b"`.
    ///
    /// The `kind == Call` discriminant is LOAD-BEARING and was once
    /// wrongly deleted on a sample-probe argument. Three probes (`return
    /// class C {}` → `any`; a `{ new (): Box }`-typed value → `{  }`;
    /// an alias to `new () => Box` → `DeclRef`) as proof that no
    /// `Construct` signature is reachable on the body-derived rail.
    /// That claim was FALSE — the annotation-typed parameter form
    /// (`function makeProps(x: new () => Box) { return x }`) measures a
    /// `SignatureKind::Construct` node through this rail's own
    /// boundary — and the deletion had let a construct signature
    /// satisfy every call-signature pin. Restored and controlled in
    /// both directions by
    /// [`expectation_controls::construct_signature_is_distinct_from_call_signature`].
    Signature {
        params: &'static [ExpectedNode],
        ret: &'static ExpectedNode,
    },
    /// A `SemanticNodeData::Signature` with `SignatureKind::Construct`
    /// (`new () => T`), same parameter/return semantics as
    /// [`ExpectedNode::Signature`]. The distinct variant is what lets a
    /// control (and a future row) pin a construct signature exactly —
    /// and REJECT a call signature where a construct one is pinned.
    ConstructSignature {
        params: &'static [ExpectedNode],
        ret: &'static ExpectedNode,
    },
    /// `SemanticNodeData::Object` whose named member set EQUALS this set
    /// (exact: a missing member and an extra member both fail), each
    /// member's value matched recursively.
    Object(&'static [(&'static str, ExpectedNode)]),
    /// `SemanticNodeData::TypeParam` with this display name — DISTINCT
    /// from `DeclRef` / `BareRef`, which project identically.
    TypeParam { name: &'static str },
    /// `SemanticNodeData::DeclRef` naming this declaration.
    DeclRef { name: &'static str },
    /// `SemanticNodeData::BareRef` carrying this unresolved name.
    BareRef { name: &'static str },
    /// The typed unmodelled-position marker
    /// (`Opaque(QueryError::UnmodeledPosition)`).
    OpaqueUnmodeledPosition,
}

/// Recursion bound for matching (cycle safety).
const MATCH_DEPTH_LIMIT: usize = 32;

fn lit_matches(expected: &Lit, got: &LiteralValue) -> bool {
    match (expected, got) {
        (Lit::Str(e), LiteralValue::String(g)) => *e == g.as_str(),
        (Lit::Num(e), LiteralValue::Number(g)) => e.to_bits() == g.to_bits(),
        _ => false,
    }
}

/// Order-insensitive EXACT set equality between measured constituents and
/// expected constituents: every expected node must claim a DISTINCT
/// measured constituent and the counts must be equal. Backtracking, so
/// duplicate expectations are handled exactly.
fn set_matches(
    dispatch: &ProjectSemanticDispatch<'_>,
    measured: &[SemanticNodeId],
    expected: &[ExpectedNode],
    depth: usize,
) -> bool {
    if measured.len() != expected.len() {
        return false;
    }
    fn assign(
        dispatch: &ProjectSemanticDispatch<'_>,
        measured: &[SemanticNodeId],
        expected: &[ExpectedNode],
        used: &mut [bool],
        index: usize,
        depth: usize,
    ) -> bool {
        if index == expected.len() {
            return true;
        }
        for (slot, candidate) in measured.iter().enumerate() {
            if !used[slot] && node_matches(dispatch, *candidate, &expected[index], depth) {
                used[slot] = true;
                if assign(dispatch, measured, expected, used, index + 1, depth) {
                    return true;
                }
                used[slot] = false;
            }
        }
        false
    }
    let mut used = vec![false; measured.len()];
    assign(dispatch, measured, expected, &mut used, 0, depth)
}

/// Whether `node` matches `expected`, recursively. Silent (no failure
/// text) — [`check_node`] wraps it with a rendered report.
pub(crate) fn node_matches(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    expected: &ExpectedNode,
    depth: usize,
) -> bool {
    if depth > MATCH_DEPTH_LIMIT {
        return false;
    }
    // NO alias deref: an `Alias` node matches nothing (see the
    // [`ExpectedNode`] docs — transparency was proven unexercised and
    // deleted; reintroduce it only together with a discriminating
    // control).
    let Some(data) = dispatch.graph().node_data(node) else {
        return false;
    };
    match (expected, data.as_ref()) {
        (ExpectedNode::Literal(lit), SemanticNodeData::Literal(value)) => lit_matches(lit, value),
        (ExpectedNode::Primitive(kind), SemanticNodeData::Primitive(got)) => kind == got,
        (ExpectedNode::Union(exp), SemanticNodeData::Union(members)) => {
            set_matches(dispatch, members, exp, depth + 1)
        }
        (ExpectedNode::Intersection(exp), SemanticNodeData::Intersection(members)) => {
            members.len() == exp.len()
                && members
                    .iter()
                    .zip(exp.iter())
                    .all(|(member, arm)| node_matches(dispatch, *member, arm, depth + 1))
        }
        (
            ExpectedNode::Signature { params, ret },
            SemanticNodeData::Signature {
                kind,
                params: got_params,
                return_type,
                ..
            },
        ) => {
            // The `kind == Call` discriminant is load-bearing: the
            // annotation-typed parameter form (`x: new () => Box`)
            // reaches a Construct signature on this rail, and a call
            // pin must reject it (held by
            // `construct_signature_is_distinct_from_call_signature`).
            // Arity is exact and the parameter types are ORDERED —
            // both clauses are held by
            // `signature_params_and_arity_reject_wrong_shapes`.
            *kind == SignatureKind::Call
                && got_params.len() == params.len()
                && got_params
                    .iter()
                    .zip(params.iter())
                    .all(|(param, exp)| node_matches(dispatch, param.ty, exp, depth + 1))
                && node_matches(dispatch, *return_type, ret, depth + 1)
        }
        (
            ExpectedNode::ConstructSignature { params, ret },
            SemanticNodeData::Signature {
                kind,
                params: got_params,
                return_type,
                ..
            },
        ) => {
            *kind == SignatureKind::Construct
                && got_params.len() == params.len()
                && got_params
                    .iter()
                    .zip(params.iter())
                    .all(|(param, exp)| node_matches(dispatch, param.ty, exp, depth + 1))
                && node_matches(dispatch, *return_type, ret, depth + 1)
        }
        (ExpectedNode::Object(exp), SemanticNodeData::Object(surface)) => {
            // INJECTIVE: every expected member must claim a DISTINCT
            // measured member, so duplicate expected keys cannot both be
            // satisfied by one actual member. Measured object surfaces
            // carry unique keys, so greedy claiming is exact.
            let members = surface.positive_members();
            if members.len() != exp.len() {
                return false;
            }
            let mut used = vec![false; members.len()];
            exp.iter().all(|(name, value)| {
                members.iter().enumerate().any(|(slot, member)| {
                    if used[slot]
                        || member.key.as_string() != Some(*name)
                        || !node_matches(dispatch, member.value, value, depth + 1)
                    {
                        return false;
                    }
                    used[slot] = true;
                    true
                })
            })
        }
        (ExpectedNode::TypeParam { name }, SemanticNodeData::TypeParam { display_name, .. }) => {
            &**display_name == *name
        }
        (ExpectedNode::DeclRef { name }, SemanticNodeData::DeclRef { identity }) => {
            &*identity.decl_name == *name
        }
        (ExpectedNode::BareRef { name }, SemanticNodeData::BareRef(_)) => {
            data.bare_ref_head().is_some_and(|(got, _)| &**got == *name)
        }
        (
            ExpectedNode::OpaqueUnmodeledPosition,
            SemanticNodeData::Opaque(QueryError::UnmodeledPosition),
        ) => true,
        _ => false,
    }
}

/// Render a graph node recursively, compactly, for dump mode and failure
/// reports. Depth-capped; the cap renders as `…`.
pub(crate) fn render_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    depth: usize,
) -> String {
    if depth > 6 {
        return "…".to_owned();
    }
    let Some(data) = dispatch.graph().node_data(node) else {
        return "<evicted>".to_owned();
    };
    match data.as_ref() {
        SemanticNodeData::Alias(inner) => {
            format!("Alias({})", render_node(dispatch, *inner, depth + 1))
        }
        SemanticNodeData::Object(surface) => {
            let members: Vec<String> = surface
                .positive_members()
                .iter()
                .map(|member| {
                    format!(
                        "{}: {}",
                        member.key.as_string().unwrap_or("<non-string-key>"),
                        render_node(dispatch, member.value, depth + 1)
                    )
                })
                .collect();
            format!("{{ {} }}", members.join(", "))
        }
        SemanticNodeData::ObjectSpreadProgram(_) => "ObjectSpreadProgram".to_owned(),
        SemanticNodeData::Union(members) => {
            let parts: Vec<String> = members
                .iter()
                .map(|member| render_node(dispatch, *member, depth + 1))
                .collect();
            format!("Union({})", parts.join(" | "))
        }
        SemanticNodeData::Intersection(members) => {
            let parts: Vec<String> = members
                .iter()
                .map(|member| render_node(dispatch, *member, depth + 1))
                .collect();
            format!("Intersection({})", parts.join(" & "))
        }
        SemanticNodeData::Primitive(kind) => format!("{kind:?}").to_ascii_lowercase(),
        SemanticNodeData::Literal(LiteralValue::String(value)) => format!("\"{value}\""),
        SemanticNodeData::Literal(LiteralValue::Number(value)) => format!("{value}"),
        SemanticNodeData::Literal(LiteralValue::Boolean(value)) => format!("{value}"),
        SemanticNodeData::Literal(LiteralValue::BigInt(value)) => format!("{value}n"),
        SemanticNodeData::Opaque(error) => {
            let text = format!("{error:?}");
            let short: String = text.chars().take(60).collect();
            format!("Opaque({short})")
        }
        SemanticNodeData::Array { element, readonly } => format!(
            "{}Array({})",
            if *readonly { "Readonly" } else { "" },
            render_node(dispatch, *element, depth + 1)
        ),
        SemanticNodeData::Tuple { .. } => "Tuple(…)".to_owned(),
        SemanticNodeData::TemplateLiteral { .. } => "TemplateLiteral(…)".to_owned(),
        SemanticNodeData::KeyOf { base } => {
            format!("KeyOf({})", render_node(dispatch, *base, depth + 1))
        }
        SemanticNodeData::IndexedAccess { .. } => "IndexedAccess(…)".to_owned(),
        SemanticNodeData::Mapped { .. } => "Mapped(…)".to_owned(),
        SemanticNodeData::TypeOf(_) => "TypeOf(…)".to_owned(),
        SemanticNodeData::TypeParam { display_name, .. } => format!("TypeParam({display_name})"),
        SemanticNodeData::Infer { name, .. } => format!("Infer({name})"),
        SemanticNodeData::InferRef { name, .. } => format!("InferRef({name})"),
        SemanticNodeData::MergedDecl { .. } => "MergedDecl(…)".to_owned(),
        SemanticNodeData::Conditional { .. } => "Conditional(…)".to_owned(),
        SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            ..
        } => {
            let rendered: Vec<String> = params
                .iter()
                .map(|param| render_node(dispatch, param.ty, depth + 1))
                .collect();
            format!(
                "{}({}) => {}",
                if *kind == SignatureKind::Construct {
                    "new "
                } else {
                    ""
                },
                rendered.join(", "),
                render_node(dispatch, *return_type, depth + 1)
            )
        }
        SemanticNodeData::DeferredCallable(_) => "DeferredCallable(…)".to_owned(),
        SemanticNodeData::DeclRef { identity } => format!("DeclRef({})", identity.decl_name),
        SemanticNodeData::InstantiationRef { base, .. } => {
            format!("InstantiationRef({})", base.decl_name)
        }
        SemanticNodeData::BareRef(_) => format!(
            "BareRef({})",
            data.bare_ref_head()
                .map(|(name, _)| name.to_string())
                .unwrap_or_else(|| "?".to_owned())
        ),
        SemanticNodeData::ImportType(_) => "ImportType(…)".to_owned(),
        SemanticNodeData::RawFallback { .. } => "RawFallback(…)".to_owned(),
        SemanticNodeData::SyntheticBinding { .. } => "SyntheticBinding(…)".to_owned(),
    }
}

/// Match a node against an expectation, returning a self-contained
/// failure list (empty on match). The failure carries both trees plus
/// the oracle/profile stamps, so the report needs no re-derivation.
pub(crate) fn check_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    expected: &ExpectedNode,
) -> Vec<String> {
    if node_matches(dispatch, node, expected, 0) {
        Vec::new()
    } else {
        vec![format!(
            "expected {expected:?}\n           measured {}\n           [oracle: {ORACLE_STAMP}]\n           [profile: {PROFILE_STAMP}]",
            render_node(dispatch, node, 0)
        )]
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Row columns: recursive expectation + public-boundary companion
// ─────────────────────────────────────────────────────────────────────────

/// The recursive-expectation column of a corpus `Row`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Expect {
    /// The row does not pin a recursive expectation.
    Skip,
    /// The row's flow-return node must match this expectation, matched at
    /// the graph-node level through the public audited boundary.
    Node(&'static ExpectedNode),
}

/// The public-boundary companion column of a corpus `Row`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Boundary {
    /// The row does not drive the public boundary.
    Skip,
    /// `get_flow_return_type_with_audit`, invoked TWICE, both calls
    /// modelled explicitly:
    ///
    /// * call 1 must produce a VALUE, be COLD (`from_cache == false`,
    ///   `cold_computes >= 1`), carry exactly `degradation`, and project
    ///   exactly `json` (`project_node_to_type_expr_json_bytes` of the
    ///   result node);
    /// * call 2 must keep call 1's result CLASS (a value, never a
    ///   refusal) and typed degradation, project byte-identically, and
    ///   report the pinned cache-replay state: `warm_replay == true`
    ///   demands a warm family replay (`from_cache == true`) with ZERO
    ///   cold computes; `warm_replay == false` demands the result was
    ///   NOT admitted warm (`from_cache == false`, the `ReturnOnly`
    ///   no-poison contract) AND that the second call genuinely
    ///   COLD-COMPUTES again (`cold_computes >= 1`) — `from_cache ==
    ///   false` with zero cold work is a replay that did nothing, not a
    ///   cold replay.
    Audit {
        json: &'static str,
        degradation: Degr,
        warm_replay: bool,
    },
    /// `get_flow_return_type_with_audit`, invoked TWICE, where the
    /// boundary must REFUSE (a typed no-value) on BOTH calls: call 1
    /// refuses COLD (`from_cache == false`, `cold_computes >= 1`) with
    /// EXACTLY the pinned typed refusal (`error` — the full
    /// [`FlowReturnError`] identity, so a refusal swapped for a
    /// different typed refusal fails); call 2 refuses AGAIN with the
    /// SAME refusal identity as call 1 (a refusal that changes kind
    /// across calls fails), is never served warm (`from_cache == false`
    /// — a cached refusal is the typed-non-admission violation this pin
    /// exists to catch), and genuinely recomputes (`cold_computes >=
    /// 1`). Checked by [`check_boundary_refusal`].
    AuditRefusal { error: FlowReturnError },
}

/// What the public boundary actually did across the two calls.
#[derive(Debug)]
pub(crate) struct MeasuredBoundary {
    pub first_from_cache: bool,
    pub first_cold_computes: u32,
    /// Typed degradation of the first call's result (`None` arm of the
    /// carrier ⇒ this is `Degr::None`-or-real; an `Err` carrier leaves it
    /// unset and records `error`).
    pub degradation: Option<Degr>,
    /// Exact projected JSON of the first call's result node.
    pub json: Option<String>,
    pub second_from_cache: bool,
    pub second_cold_computes: u32,
    /// Typed degradation of the SECOND call's result — the replay must
    /// carry the same typed degradation as the first call, so drift here
    /// is a comparison failure, not an unrecorded fact.
    pub second_degradation: Option<Degr>,
    /// Exact projected JSON of the second call's result node.
    pub second_json: Option<String>,
    /// Debug of the `Err` arm when the FIRST call produced no value.
    pub error: Option<String>,
    /// The TYPED `Err` arm of the first call — the refusal identity the
    /// kind pin and the cross-call identity clause compare. The debug
    /// string above is presentation; this field is the semantics.
    pub error_kind: Option<FlowReturnError>,
    /// Debug of the `Err` arm when the SECOND call produced no value —
    /// recorded separately so a result-class drift across the replay
    /// (value ⇄ refusal) is comparable, never discarded.
    pub second_error: Option<String>,
    /// The TYPED `Err` arm of the second call — compared against
    /// `error_kind` so a refusal that changes kind across calls is a
    /// comparison failure, not an unrecorded fact.
    pub second_error_kind: Option<FlowReturnError>,
}

/// A full expect + boundary measurement of one program.
pub(crate) struct LaneMeasurement {
    pub boundary: MeasuredBoundary,
    /// `Some(failures)` when an expectation was supplied.
    pub expect_failures: Option<Vec<String>>,
    /// Compact recursive rendering of the result node.
    pub rendered: Option<String>,
}

pub(crate) fn make_audit_host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone_with_scheduler_config(
        HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: false,
            ..HostConfig::default()
        },
        verter_scheduler::scheduler::SchedulerConfig {
            cpu_threads: 1,
            ..verter_scheduler::scheduler::SchedulerConfig::default()
        },
    ))
}

fn identity(canonical: &str, symbol: &str) -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

/// Drive one program through the PUBLIC audited flow-return boundary
/// twice, and (optionally) match the result node against a recursive
/// expectation while the host's graph is live.
pub(crate) fn drive_expect_boundary(
    aux: &str,
    id: &str,
    script: &str,
    function: &str,
    expected: Option<&ExpectedNode>,
) -> LaneMeasurement {
    let host = make_audit_host();
    let dir = "/wb";
    if !aux.is_empty() {
        upsert(
            &host,
            &format!("{dir}/{id}__aux.ts"),
            aux,
            FileLanguage::script_ts(),
        );
    }
    let canonical = format!("{dir}/{id}.ts");
    upsert(&host, &canonical, script, FileLanguage::script_ts());
    let ident = identity(&canonical, function);

    let first =
        host.get_flow_return_type_with_audit(&ident, ReturnProjectionDemand::whole_return());
    let first_record = first.audit();
    let first_payload = first_record
        .flow_return_inference_payload()
        .expect("FlowReturnInference record carries the flow payload");
    let first_from_cache = first_record.from_cache;
    let first_cold_computes = first_payload.cold_computes;

    let (degradation, json, node, error, error_kind) = match first.as_result() {
        Ok(result) => (
            Some(degr_of(result.degradation())),
            host.project_node_to_type_expr_json_bytes(result.return_type())
                .map(|bytes| String::from_utf8(bytes).expect("TypeExpr JSON is UTF-8")),
            Some(result.return_type()),
            None,
            None,
        ),
        Err(err) => (None, None, None, Some(format!("{err:?}")), Some(*err)),
    };

    // Match + render against the LIVE graph before the host drops.
    let (expect_failures, rendered) = {
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        match node {
            Some(node) => (
                expected.map(|exp| check_node(&dispatch, node, exp)),
                Some(render_node(&dispatch, node, 0)),
            ),
            None => (
                expected.map(|exp| {
                    vec![format!(
                        "expected {exp:?}, but the public boundary returned no value: {}",
                        error.as_deref().unwrap_or("<unset>")
                    )]
                }),
                None,
            ),
        }
    };

    let second =
        host.get_flow_return_type_with_audit(&ident, ReturnProjectionDemand::whole_return());
    let second_record = second.audit();
    let second_payload = second_record
        .flow_return_inference_payload()
        .expect("FlowReturnInference record carries the flow payload");
    let (second_degradation, second_json, second_error, second_error_kind) =
        match second.as_result() {
            Ok(result) => (
                Some(degr_of(result.degradation())),
                host.project_node_to_type_expr_json_bytes(result.return_type())
                    .map(|bytes| String::from_utf8(bytes).expect("TypeExpr JSON is UTF-8")),
                None,
                None,
            ),
            Err(err) => (None, None, Some(format!("{err:?}")), Some(*err)),
        };

    LaneMeasurement {
        boundary: MeasuredBoundary {
            first_from_cache,
            first_cold_computes,
            degradation,
            json,
            second_from_cache: second_record.from_cache,
            second_cold_computes: second_payload.cold_computes,
            second_degradation,
            second_json,
            error,
            error_kind,
            second_error,
            second_error_kind,
        },
        expect_failures,
        rendered,
    }
}

/// The SECOND-call replay clauses, shared verbatim by
/// [`check_boundary`] (corpus rows) and [`matrix::check_cell_outcome`]
/// (matrix `Value` cells) so the two comparators cannot drift apart:
/// result-class drift, typed-degradation drift, the warm pair (both
/// clauses, individually), the cold pair (both clauses, individually),
/// and replay-projection drift. Pure over the measurement.
fn replay_clauses(warm_replay: bool, measured: &MeasuredBoundary) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(err) = &measured.second_error {
        failures.push(format!(
            "call 2 returned NO VALUE ({err}) where call 1 produced a result — the replay \
             changed the result CLASS"
        ));
        return failures;
    }
    if measured.second_degradation != measured.degradation {
        failures.push(format!(
            "typed degradation drifted across the replay: call 1 measured {:?}, call 2 \
             measured {:?}",
            measured.degradation, measured.second_degradation
        ));
    }
    if warm_replay {
        if !measured.second_from_cache {
            failures.push(
                "call 2 reported from_cache=false — a clean result must REPLAY WARM".to_owned(),
            );
        }
        if measured.second_cold_computes != 0 {
            failures.push(format!(
                "call 2 ran {} cold evaluations — a warm family replay runs ZERO",
                measured.second_cold_computes
            ));
        }
    } else {
        if measured.second_from_cache {
            failures.push(
                "call 2 reported from_cache=true — this result is ReturnOnly and must NOT be \
                 admitted warm; a warm replay here is the no-poison violation the pin exists \
                 to catch"
                    .to_owned(),
            );
        }
        if measured.second_cold_computes == 0 {
            failures.push(
                "call 2 ran ZERO cold evaluations — a result that must not be admitted warm \
                 has to COLD-COMPUTE again on the second call; from_cache=false with zero \
                 cold work is a replay that did nothing"
                    .to_owned(),
            );
        }
    }
    match (&measured.json, &measured.second_json) {
        (Some(first), Some(second)) if first == second => {}
        (first, second) => failures.push(format!(
            "replay projection drifted: call 1 projected\n  {}\ncall 2 projected\n  {}",
            first.as_deref().unwrap_or("<no projection>"),
            second.as_deref().unwrap_or("<no projection>")
        )),
    }
    failures
}

/// Drive one program through the PUBLIC audited flow-return boundary
/// ONCE and hand the caller the LIVE dispatch plus the result node
/// (`None` when the boundary refused). Shared by the expectation
/// controls and the checker-syntax semantic cross-validation.
pub(crate) fn with_live_flow_node<R>(
    aux: &str,
    id: &str,
    script: &str,
    function: &str,
    f: impl FnOnce(&ProjectSemanticDispatch<'_>, Option<SemanticNodeId>) -> R,
) -> R {
    let host = make_audit_host();
    let dir = "/wb";
    if !aux.is_empty() {
        upsert(
            &host,
            &format!("{dir}/{id}__aux.ts"),
            aux,
            FileLanguage::script_ts(),
        );
    }
    let canonical = format!("{dir}/{id}.ts");
    upsert(&host, &canonical, script, FileLanguage::script_ts());
    let carrier = host.get_flow_return_type_with_audit(
        &identity(&canonical, function),
        ReturnProjectionDemand::whole_return(),
    );
    let node = carrier.as_result().ok().map(|result| result.return_type());
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    f(&dispatch, node)
}

/// The two FIRST-CALL-COLD clauses, shared verbatim by
/// [`check_boundary`] (corpus rows) and
/// [`matrix::check_cell_outcome_measured`]'s `Value` arm (matrix
/// cells), so a matrix cell pins first-call coldness at parity with a
/// corpus row. Pure over the measurement.
fn first_call_cold_clauses(measured: &MeasuredBoundary) -> Vec<String> {
    let mut failures = Vec::new();
    if measured.first_from_cache {
        failures.push(
            "call 1 reported from_cache=true — the first call on a fresh host must be COLD"
                .to_owned(),
        );
    }
    if measured.first_cold_computes == 0 {
        failures.push(
            "call 1 reported cold_computes == 0 — a cold evaluation must count at least one"
                .to_owned(),
        );
    }
    failures
}

/// Check a measured public-boundary trace against a [`Boundary::Audit`]
/// pin. Pure over the measurement, so the negative controls can prove
/// every clause is able to fail.
pub(crate) fn check_boundary(
    json: &str,
    degradation: Degr,
    warm_replay: bool,
    measured: &MeasuredBoundary,
) -> Vec<String> {
    let mut failures = Vec::new();
    if let Some(error) = &measured.error {
        failures.push(format!(
            "the public boundary returned NO VALUE ({error}) where the pin expects a result"
        ));
        return failures;
    }
    failures.extend(first_call_cold_clauses(measured));
    match measured.degradation {
        Some(got) if got == degradation => {}
        got => failures.push(format!(
            "typed degradation: expected {degradation:?}, measured {got:?}"
        )),
    }
    match &measured.json {
        Some(got) if got == json => {}
        got => failures.push(format!(
            "projected JSON of call 1: expected EXACTLY\n  {json}\nmeasured\n  {}",
            got.as_deref().unwrap_or("<no projection>")
        )),
    }
    failures.extend(replay_clauses(warm_replay, measured));
    failures
}

/// Check a measured public-boundary trace against a
/// [`Boundary::AuditRefusal`] pin: the boundary must REFUSE on both
/// calls with EXACTLY the pinned typed refusal, keep the same refusal
/// identity across the two calls, refuse cold each time, and never be
/// admitted warm. Pure over the measurement, so the negative controls
/// can prove every clause is able to fail.
pub(crate) fn check_boundary_refusal(
    expected: FlowReturnError,
    measured: &MeasuredBoundary,
) -> Vec<String> {
    let mut failures = Vec::new();
    if measured.error.is_none() {
        failures.push(format!(
            "call 1 produced a VALUE (projection {:?}, degradation {:?}) where the pin \
             expects a typed no-value refusal",
            measured.json, measured.degradation
        ));
        return failures;
    }
    match measured.error_kind {
        Some(got) if got == expected => {}
        got => failures.push(format!(
            "typed refusal KIND of call 1: the pin demands EXACTLY {expected:?}, measured \
             {got:?} — a refusal swapped for a different typed refusal is a contract \
             change, not still-a-refusal"
        )),
    }
    if measured.first_from_cache {
        failures.push(
            "call 1 reported from_cache=true — a refusal on a fresh host must be COLD".to_owned(),
        );
    }
    if measured.first_cold_computes == 0 {
        failures.push(
            "call 1 reported cold_computes == 0 — a genuine refusal is computed, not conjured"
                .to_owned(),
        );
    }
    if measured.second_error.is_none() {
        failures.push(format!(
            "call 2 produced a VALUE (projection {:?}) where call 1 refused — the replay \
             changed the result CLASS",
            measured.second_json
        ));
    } else if measured.second_error_kind != measured.error_kind {
        failures.push(format!(
            "refusal IDENTITY drifted across the replay: call 1 refused {:?}, call 2 \
             refused {:?} — a refusal that changes kind between calls is two different \
             contracts wearing one pin",
            measured.error_kind, measured.second_error_kind
        ));
    }
    if measured.second_from_cache {
        failures.push(
            "call 2 reported from_cache=true — a refusal must NEVER be admitted warm; a \
             cached refusal is the typed-non-admission violation this pin exists to catch"
                .to_owned(),
        );
    }
    if measured.second_cold_computes == 0 {
        failures.push(
            "call 2 ran ZERO cold evaluations — a non-admitted refusal must genuinely \
             RECOMPUTE on every demand"
                .to_owned(),
        );
    }
    failures
}

// ─────────────────────────────────────────────────────────────────────────
// Checker-syntax semantic projection
// ─────────────────────────────────────────────────────────────────────────

/// A TYPED, test-only projection of the pinned checker's PRINT syntax,
/// with ONE canonical comparison rule against the live semantic graph.
///
/// This is what makes a deep-pinned row's `checker` column LOAD-BEARING:
/// `deep_pinned_rows_semantic_equality_follows_their_verdict` parses the
/// column into [`CheckerType`] and compares it against the LIVE graph
/// for EVERY deep-pinned row, independent of the `RENDER_COMPARABLE` /
/// `RENDER_INCOMPARABLE` byte lists — so a `RENDER_INCOMPARABLE` entry
/// exempts PRESENTATION BYTES only, never semantic equality.
///
/// The grammar covers exactly the print forms the deep-pinned rows use:
/// string and number literals, primitive names, bare reference names,
/// `A | B` unions, `A & B` intersections with parentheses, object
/// prints `{ name: T; }` (nested to arbitrary depth), and
/// `(p: T, …) => T` function prints. An unsupported construct is a
/// LOUD parse error naming the offending text — never a silent
/// exemption; extend the parser (with its comparison rule and a
/// control) when a new deep-pinned row needs a new form.
///
/// Comparison rules, canonical on purpose: unions are order-INSENSITIVE
/// exact constituent sets (the checker prints its own internal order);
/// intersections are ORDERED (source order on both sides); objects are
/// exact member sets; a function print is a CALL signature only (the
/// checker prints a construct signature `new (…) => T`, which this
/// grammar does not model — a construct-signature node therefore never
/// satisfies a function print, and a construct-bearing deep row must
/// extend the grammar WITH a control); function parameter NAMES are
/// ignored (they are print artifacts, not semantics) while parameter
/// TYPES and arity are exact; a reference name matches a RESOLVED
/// `DeclRef` carrying that name ONLY. The former `BareRef` / `TypeParam`
/// acceptance arms were DELETED as unexercised comparison vocabulary
/// (no deep-pinned row and no control measures either through this
/// rail; the deletion removes ACCEPT arms, so every such pair now
/// reaches the controlled `_ => false` — fail-closed, exhaustive over
/// the match arms). A future deep-pinned row whose graph node is a
/// `BareRef`/`TypeParam` fails loudly here and must reintroduce the
/// arm together with a discriminating control; the graph-level
/// [`ExpectedNode`] pins keep holding reference identity exactly.
pub(crate) mod checker_syntax {
    use super::*;

    /// The typed form a `checker` column parses into.
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) enum CheckerType {
        StringLit(String),
        NumberLit(f64),
        Primitive(PrimitiveKind),
        Ref(String),
        Union(Vec<CheckerType>),
        Intersection(Vec<CheckerType>),
        Object(Vec<(String, CheckerType)>),
        Function {
            params: Vec<CheckerType>,
            ret: Box<CheckerType>,
        },
    }

    /// Parse a checker print into its typed form. Strict: trailing
    /// input, unsupported constructs, and malformed prints are ERRORS.
    pub(crate) fn parse(text: &str) -> Result<CheckerType, String> {
        let mut parser = Parser { text, pos: 0 };
        let parsed = parser.union()?;
        parser.skip_ws();
        if parser.pos != text.len() {
            return Err(format!(
                "trailing input at byte {} of `{text}` — extend the checker-syntax parser \
                 deliberately, never exempt silently",
                parser.pos
            ));
        }
        Ok(parsed)
    }

    struct Parser<'a> {
        text: &'a str,
        pos: usize,
    }

    impl<'a> Parser<'a> {
        fn rest(&self) -> &'a str {
            &self.text[self.pos..]
        }

        fn skip_ws(&mut self) {
            while self.rest().starts_with(|c: char| c.is_ascii_whitespace()) {
                self.pos += 1;
            }
        }

        fn eat(&mut self, token: char) -> bool {
            self.skip_ws();
            if self.rest().starts_with(token) {
                self.pos += token.len_utf8();
                true
            } else {
                false
            }
        }

        fn expect(&mut self, token: char) -> Result<(), String> {
            if self.eat(token) {
                Ok(())
            } else {
                Err(format!(
                    "expected `{token}` at byte {} of `{}`",
                    self.pos, self.text
                ))
            }
        }

        fn union(&mut self) -> Result<CheckerType, String> {
            let mut parts = vec![self.intersection()?];
            while self.eat('|') {
                parts.push(self.intersection()?);
            }
            Ok(if parts.len() == 1 {
                parts.pop().expect("one part")
            } else {
                CheckerType::Union(parts)
            })
        }

        fn intersection(&mut self) -> Result<CheckerType, String> {
            let mut parts = vec![self.atom()?];
            while self.eat('&') {
                parts.push(self.atom()?);
            }
            Ok(if parts.len() == 1 {
                parts.pop().expect("one part")
            } else {
                CheckerType::Intersection(parts)
            })
        }

        fn ident(&mut self) -> Result<String, String> {
            self.skip_ws();
            let rest = self.rest();
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '$')
                .unwrap_or(rest.len());
            if end == 0 {
                return Err(format!(
                    "expected an identifier at byte {} of `{}`",
                    self.pos, self.text
                ));
            }
            self.pos += end;
            Ok(rest[..end].to_owned())
        }

        fn atom(&mut self) -> Result<CheckerType, String> {
            self.skip_ws();
            let rest = self.rest();
            match rest.chars().next() {
                Some('(') => {
                    // `(p: T, …) => R` or a parenthesised type — try the
                    // function print first, backtrack on failure.
                    let save = self.pos;
                    match self.function() {
                        Ok(function) => Ok(function),
                        Err(_) => {
                            self.pos = save;
                            self.expect('(')?;
                            let inner = self.union()?;
                            self.expect(')')?;
                            Ok(inner)
                        }
                    }
                }
                Some('{') => self.object(),
                Some('"') => {
                    let inner = &rest[1..];
                    let close = inner
                        .find('"')
                        .ok_or_else(|| format!("unterminated string literal in `{}`", self.text))?;
                    if inner[..close].contains('\\') {
                        return Err(format!(
                            "escaped string literals are not modelled (`{}`)",
                            self.text
                        ));
                    }
                    self.pos += close + 2;
                    Ok(CheckerType::StringLit(inner[..close].to_owned()))
                }
                Some(c) if c.is_ascii_digit() || c == '-' => {
                    let end = rest[1..]
                        .find(|c: char| !c.is_ascii_digit() && c != '.')
                        .map(|offset| offset + 1)
                        .unwrap_or(rest.len());
                    let value: f64 = rest[..end].parse().map_err(|err| {
                        format!("malformed number literal `{}`: {err}", &rest[..end])
                    })?;
                    self.pos += end;
                    Ok(CheckerType::NumberLit(value))
                }
                _ => {
                    let name = self.ident()?;
                    Ok(match name.as_str() {
                        "string" => CheckerType::Primitive(PrimitiveKind::String),
                        "number" => CheckerType::Primitive(PrimitiveKind::Number),
                        "boolean" => CheckerType::Primitive(PrimitiveKind::Boolean),
                        "symbol" => CheckerType::Primitive(PrimitiveKind::Symbol),
                        "bigint" => CheckerType::Primitive(PrimitiveKind::BigInt),
                        "undefined" => CheckerType::Primitive(PrimitiveKind::Undefined),
                        "null" => CheckerType::Primitive(PrimitiveKind::Null),
                        "void" => CheckerType::Primitive(PrimitiveKind::Void),
                        "never" => CheckerType::Primitive(PrimitiveKind::Never),
                        "unknown" => CheckerType::Primitive(PrimitiveKind::Unknown),
                        "any" => CheckerType::Primitive(PrimitiveKind::Any),
                        "object" => CheckerType::Primitive(PrimitiveKind::Object),
                        "true" | "false" => {
                            return Err(format!(
                                "boolean literal prints are not modelled (`{}`) — add the \
                                 variant WITH its comparison rule and control when a deep \
                                 row needs it",
                                self.text
                            ));
                        }
                        _ => CheckerType::Ref(name),
                    })
                }
            }
        }

        fn object(&mut self) -> Result<CheckerType, String> {
            self.expect('{')?;
            let mut members = Vec::new();
            loop {
                if self.eat('}') {
                    return Ok(CheckerType::Object(members));
                }
                let name = self.ident()?;
                if self.eat('?') {
                    return Err(format!(
                        "optional members are not modelled (`{}`) — add the optionality \
                         comparison rule WITH a control when a deep row needs it",
                        self.text
                    ));
                }
                self.expect(':')?;
                let value = self.union()?;
                self.expect(';')?;
                members.push((name, value));
            }
        }

        fn function(&mut self) -> Result<CheckerType, String> {
            self.expect('(')?;
            let mut params = Vec::new();
            self.skip_ws();
            if !self.eat(')') {
                loop {
                    // Parameter NAMES are print artifacts; only the type
                    // is compared.
                    let _name = self.ident()?;
                    if self.eat('?') {
                        return Err(format!(
                            "optional parameters are not modelled (`{}`)",
                            self.text
                        ));
                    }
                    self.expect(':')?;
                    params.push(self.union()?);
                    if self.eat(',') {
                        continue;
                    }
                    self.expect(')')?;
                    break;
                }
            }
            self.skip_ws();
            if !self.rest().starts_with("=>") {
                return Err(format!(
                    "expected `=>` at byte {} of `{}`",
                    self.pos, self.text
                ));
            }
            self.pos += 2;
            let ret = self.union()?;
            Ok(CheckerType::Function {
                params,
                ret: Box::new(ret),
            })
        }
    }

    /// Whether the LIVE graph node semantically equals the parsed
    /// checker form, under the canonical comparison rules above. No
    /// alias deref, mirroring [`node_matches`].
    pub(crate) fn matches_node(
        dispatch: &ProjectSemanticDispatch<'_>,
        node: SemanticNodeId,
        expected: &CheckerType,
        depth: usize,
    ) -> bool {
        if depth > MATCH_DEPTH_LIMIT {
            return false;
        }
        let Some(data) = dispatch.graph().node_data(node) else {
            return false;
        };
        match (expected, data.as_ref()) {
            (CheckerType::StringLit(e), SemanticNodeData::Literal(LiteralValue::String(g))) => {
                e == g.as_str()
            }
            (CheckerType::NumberLit(e), SemanticNodeData::Literal(LiteralValue::Number(g))) => {
                e.to_bits() == g.to_bits()
            }
            (CheckerType::Primitive(e), SemanticNodeData::Primitive(g)) => e == g,
            // A reference name matches a RESOLVED `DeclRef` only. The
            // former `BareRef` / `TypeParam` acceptance arms were
            // deleted as unexercised (see the module doc): those pairs
            // now reach the `_ => false` catchall, fail-closed.
            (CheckerType::Ref(name), SemanticNodeData::DeclRef { identity }) => {
                &*identity.decl_name == name.as_str()
            }
            (CheckerType::Union(exp), SemanticNodeData::Union(members)) => {
                // Order-insensitive EXACT set equality, backtracking so
                // duplicate constituents are handled exactly.
                if members.len() != exp.len() {
                    return false;
                }
                fn assign(
                    dispatch: &ProjectSemanticDispatch<'_>,
                    measured: &[SemanticNodeId],
                    expected: &[CheckerType],
                    used: &mut [bool],
                    index: usize,
                    depth: usize,
                ) -> bool {
                    if index == expected.len() {
                        return true;
                    }
                    for (slot, candidate) in measured.iter().enumerate() {
                        if !used[slot]
                            && matches_node(dispatch, *candidate, &expected[index], depth)
                        {
                            used[slot] = true;
                            if assign(dispatch, measured, expected, used, index + 1, depth) {
                                return true;
                            }
                            used[slot] = false;
                        }
                    }
                    false
                }
                let mut used = vec![false; members.len()];
                assign(dispatch, members, exp, &mut used, 0, depth + 1)
            }
            (CheckerType::Intersection(exp), SemanticNodeData::Intersection(members)) => {
                members.len() == exp.len()
                    && members
                        .iter()
                        .zip(exp.iter())
                        .all(|(member, arm)| matches_node(dispatch, *member, arm, depth + 1))
            }
            (CheckerType::Object(exp), SemanticNodeData::Object(surface)) => {
                let members = surface.positive_members();
                if members.len() != exp.len() {
                    return false;
                }
                let mut used = vec![false; members.len()];
                exp.iter().all(|(name, value)| {
                    members.iter().enumerate().any(|(slot, member)| {
                        if used[slot]
                            || member.key.as_string() != Some(name.as_str())
                            || !matches_node(dispatch, member.value, value, depth + 1)
                        {
                            return false;
                        }
                        used[slot] = true;
                        true
                    })
                })
            }
            (
                CheckerType::Function { params, ret },
                SemanticNodeData::Signature {
                    kind,
                    params: got_params,
                    return_type,
                    ..
                },
            ) => {
                // A function print is a CALL signature: the checker
                // prints construct signatures `new (…) => T`, which
                // this grammar does not model, so a Construct node must
                // never satisfy a Function print (the restored
                // discriminant — see the module doc).
                *kind == SignatureKind::Call
                    && got_params.len() == params.len()
                    && got_params
                        .iter()
                        .zip(params.iter())
                        .all(|(param, exp)| matches_node(dispatch, param.ty, exp, depth + 1))
                    && matches_node(dispatch, *return_type, ret, depth + 1)
            }
            _ => false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The crossed capture-write matrix
// ─────────────────────────────────────────────────────────────────────────

/// The crossed capture-write / effect / completion position matrix.
///
/// Axes (§7 of `docs/arch/u6-flow-return-gaps-and-target.md`): binding
/// kind × write timing × closure depth × expression position × guard
/// kind × completion container. Cells are generated from ONE shared
/// program generator so a position can never drift from its siblings,
/// and each cell records the pinned checker answer for its program
/// ([`ORACLE_STAMP`]) plus the pinned CURRENT substrate outcome.
///
/// The KEY assertion is [`matrix_suite::same_capture_write_cell_is_position_independent`]:
/// for the invoked-IIFE capture-write cell, the measured outcome must be
/// IDENTICAL across every covered expression position. A change that
/// wires a capture-write effect at one position and not its siblings
/// breaks that uniformity and fails loudly.
pub(crate) mod matrix {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum BindingKind {
        Let,
        Var,
        Const,
        Param,
    }

    /// Deliberately TOTAL — the §7 axis vocabulary exists ahead of the
    /// cells that will cover it; an uncovered variant is a named gap,
    /// not an error.
    #[allow(dead_code)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum WriteTiming {
        BeforeCreation,
        AfterCreation,
        /// The write runs inside an IMMEDIATELY-INVOKED closure.
        InsideInvokedIife,
        /// The write runs inside the RETURNED closure itself.
        InsideReturnedClosure,
        /// A sibling closure (never invoked in-body) writes the capture.
        SiblingClosure,
        /// A closure nested one level deeper writes the capture.
        DeeperClosure,
        Never,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum GuardKind {
        None,
        TypeofGuard,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum Container {
        None,
        If,
        Switch,
        TryFinally,
        Labeled,
    }

    /// Expression positions the shared generator can place the
    /// capture-write IIFE into.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum ExprPosition {
        Statement,
        DeclaratorInit,
        IfTest,
        SequenceOperand,
        CallArgument,
        Template,
        ShortCircuit,
        ObjectLiteral,
    }

    impl ExprPosition {
        pub(crate) const fn id(self) -> &'static str {
            match self {
                Self::Statement => "statement",
                Self::DeclaratorInit => "declarator_init",
                Self::IfTest => "if_test",
                Self::SequenceOperand => "sequence_operand",
                Self::CallArgument => "call_argument",
                Self::Template => "template",
                Self::ShortCircuit => "short_circuit",
                Self::ObjectLiteral => "object_literal",
            }
        }
    }

    /// The one capture-write expression every position embeds.
    pub(crate) const CAPTURE_WRITE: &str = "(() => { x = \"b\"; return true })()";

    /// The typed refusal every refusing invoked-IIFE capture-write cell
    /// MEASURES today: the directly-invoked closure statement's captured
    /// flow effect is not represented by the sequential statement
    /// evaluator. Pinned per-cell (a cell measuring any other refusal
    /// kind fails its pin), named once because the measured kind is the
    /// same for every refusing cell.
    pub(crate) const IIFE_EFFECT_REFUSAL: FlowReturnError = FlowReturnError::Failure(
        FlowReturnFailure::Unsupported(FlowReturnUnsupported::InvokedClosureEffect),
    );

    /// The statement the position axis wraps around [`CAPTURE_WRITE`].
    pub(crate) fn iife_write_statement(position: ExprPosition) -> String {
        match position {
            ExprPosition::Statement => format!("{CAPTURE_WRITE};"),
            ExprPosition::DeclaratorInit => format!("const u = {CAPTURE_WRITE}; void u;"),
            ExprPosition::IfTest => format!("if ({CAPTURE_WRITE}) {{ }}"),
            ExprPosition::SequenceOperand => format!("({CAPTURE_WRITE}, 0);"),
            ExprPosition::CallArgument => format!("sink({CAPTURE_WRITE});"),
            ExprPosition::Template => format!("const t = `${{{CAPTURE_WRITE}}}`; void t;"),
            ExprPosition::ShortCircuit => format!("const s = true && {CAPTURE_WRITE}; void s;"),
            ExprPosition::ObjectLiteral => format!("const o = {{ k: {CAPTURE_WRITE} }}; void o;"),
        }
    }

    /// The invoked-IIFE capture-write program for one expression
    /// position: `let x` seeded `"a"`, the write embedded at `position`,
    /// `x` returned.
    pub(crate) fn iife_position_program(position: ExprPosition) -> String {
        format!(
            "function sink(v: boolean) {{ }}\nfunction makeProps() {{ let x: \"a\" | \"b\" = \
             \"a\"; {} return x }}",
            iife_write_statement(position)
        )
    }

    /// One pinned outcome of a cell program under the profile stamp.
    /// Deliberately TOTAL — see [`WriteTiming`].
    #[allow(dead_code)]
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum CellOutcome {
        /// The boundary produced a value: this exact recursive rendering,
        /// this typed degradation, a COLD first call, and this
        /// second-call replay state.
        Value {
            rendered: &'static str,
            degradation: Degr,
            warm_replay: bool,
        },
        /// The boundary produced a typed no-value refusal — EXACTLY this
        /// one, on both calls (the same [`check_boundary_refusal`] model
        /// the corpus refusal rows pin).
        NoValue { error: FlowReturnError },
    }

    /// What a cell asserts across its covered positions.
    /// Deliberately TOTAL — see [`WriteTiming`].
    #[allow(dead_code)]
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum CellExpectation {
        /// Every covered position must measure EXACTLY this outcome.
        Uniform(CellOutcome),
        /// CURRENT behaviour is position-dependent: each position's
        /// outcome is pinned individually. This is a recorded,
        /// deliberately visible expected-versus-actual gap — the checker
        /// types every position identically (`checker`), so the moment an
        /// owner makes the effect position-independent every divergent
        /// pin here fails and forces a re-pin to `Uniform`.
        PositionDependent(&'static [(ExprPosition, CellOutcome)], &'static str),
    }

    /// A single-program cell (the non-position axes).
    pub(crate) struct FixedCell {
        pub id: &'static str,
        pub binding: BindingKind,
        pub timing: WriteTiming,
        pub depth: u8,
        pub guard: GuardKind,
        pub container: Container,
        pub script: &'static str,
        /// Pinned checker answer for `ReturnType<typeof makeProps>` under
        /// [`ORACLE_STAMP`].
        pub checker: &'static str,
        pub outcome: CellOutcome,
        /// `""` when the pinned outcome AGREES with the checker; otherwise
        /// the recorded expected-versus-actual gap — what diverges, who
        /// owns it — kept beside the pin so the divergence is visible in
        /// the cell itself, not recoverable only by comparing columns.
        pub gap: &'static str,
    }

    impl FixedCell {
        /// The cell's full matrix coordinate, for failure reports.
        pub(crate) fn coords(&self) -> String {
            format!(
                "binding={:?} timing={:?} depth={} guard={:?} container={:?}",
                self.binding, self.timing, self.depth, self.guard, self.container
            )
        }
    }

    /// The invoked-IIFE capture-write POSITION cell: one (binding,
    /// timing, guard, container) coordinate crossed over every covered
    /// expression position.
    pub(crate) struct PositionCell {
        pub id: &'static str,
        pub binding: BindingKind,
        pub timing: WriteTiming,
        pub depth: u8,
        pub guard: GuardKind,
        pub container: Container,
        pub positions: &'static [ExprPosition],
        /// Pinned checker answer — IDENTICAL for every position.
        pub checker: &'static str,
        pub expectation: CellExpectation,
    }

    impl PositionCell {
        /// The cell's full matrix coordinate, for failure reports.
        pub(crate) fn coords(&self) -> String {
            format!(
                "binding={:?} timing={:?} depth={} guard={:?} container={:?}",
                self.binding, self.timing, self.depth, self.guard, self.container
            )
        }
    }

    pub(crate) const COVERED_POSITIONS: &[ExprPosition] = &[
        ExprPosition::Statement,
        ExprPosition::DeclaratorInit,
        ExprPosition::IfTest,
        ExprPosition::SequenceOperand,
        ExprPosition::CallArgument,
        ExprPosition::Template,
        ExprPosition::ShortCircuit,
        ExprPosition::ObjectLiteral,
    ];

    /// Measure one cell program through the public boundary — the FULL
    /// two-call trace, so the comparator can model both calls.
    pub(crate) fn measure_cell(id: &str, script: &str) -> LaneMeasurement {
        drive_expect_boundary("", id, script, "makeProps", None)
    }

    /// Compare one measured program against a pinned [`CellOutcome`].
    ///
    /// A `Value` pin models BOTH calls: the class, the exact rendering
    /// and typed degradation of call 1, BOTH first-call-cold clauses
    /// (shared verbatim with `check_boundary`), and the full second-call
    /// replay clause set shared with `check_boundary` (class drift,
    /// degradation drift, the warm/cold pair, projection drift). A
    /// `NoValue` pin is the explicit refusal model of
    /// [`check_boundary_refusal`]: both calls refuse with exactly the
    /// pinned typed refusal, cold each time, never warm-admitted.
    ///
    /// DELIBERATE remaining delta versus a corpus [`Boundary::Audit`]
    /// row, stated precisely: a cell pins NO exact-projected-JSON
    /// constant. The cell's value-exactness pin is the exact recursive
    /// RENDERING of the same result node (complete for every cell
    /// program — no cell rendering reaches the renderer's depth cap or
    /// an opaque print), and call-2-versus-call-1 projection drift is
    /// still asserted measured-vs-measured by the shared replay
    /// clauses. The JSON constant additionally pins the `TypeExpr`
    /// projection ENCODING, which is the corpus rows' concern
    /// (boundary-encoding stability on real rows), not the matrix's
    /// (position/timing uniformity of the same few shapes) — duplicating
    /// 20 encoding constants here would pin the encoder 20 more times
    /// without discriminating any additional cell semantics.
    pub(crate) fn check_cell_outcome(
        cell_id: &str,
        coords: &str,
        position: Option<ExprPosition>,
        script: &str,
        checker: &'static str,
        pinned: &CellOutcome,
        failures: &mut Vec<String>,
    ) {
        let position_id = position.map(|p| p.id()).unwrap_or("-");
        let measured = measure_cell(&format!("{cell_id}__{position_id}"), script);
        check_cell_outcome_measured(
            cell_id, coords, position, script, checker, pinned, &measured, failures,
        );
    }

    /// The PURE comparator half of [`check_cell_outcome`] — takes the
    /// measurement instead of driving it, so the negative controls can
    /// feed single-field-substituted real traces through the exact
    /// production comparison path (including the refusal delegation).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_cell_outcome_measured(
        cell_id: &str,
        coords: &str,
        position: Option<ExprPosition>,
        script: &str,
        checker: &'static str,
        pinned: &CellOutcome,
        measured: &LaneMeasurement,
        failures: &mut Vec<String>,
    ) {
        let position_id = position.map(|p| p.id()).unwrap_or("-");
        let rendered = measured.rendered.as_deref();
        let no_value = measured.boundary.error.is_some();
        let describe = |failures: &mut Vec<String>, what: String| {
            failures.push(format!(
                "\n┌── matrix cell {cell_id} [position {position_id}]\n\
                 │ AXES     {coords}\n\
                 │ SCRIPT   {}\n\
                 │ CHECKER  {checker}\n\
                 │ {what}\n\
                 │ [oracle: {ORACLE_STAMP}]\n\
                 │ [profile: {PROFILE_STAMP}]\n\
                 └──",
                script.replace('\n', "  ⏎  ")
            ));
        };
        match pinned {
            CellOutcome::NoValue { error } => {
                if !no_value {
                    describe(
                        failures,
                        format!(
                            "EXPECTED a typed no-value refusal; MEASURED a value: {} (degr {:?})",
                            rendered.unwrap_or("<none>"),
                            measured.boundary.degradation
                        ),
                    );
                    return;
                }
                for failure in check_boundary_refusal(*error, &measured.boundary) {
                    describe(failures, failure);
                }
            }
            CellOutcome::Value {
                rendered: want,
                degradation: want_degr,
                warm_replay,
            } => {
                if no_value {
                    describe(
                        failures,
                        format!("EXPECTED a value rendering {want}; MEASURED a no-value refusal"),
                    );
                    return;
                }
                if rendered != Some(*want) {
                    describe(
                        failures,
                        format!(
                            "EXPECTED rendering {want}; MEASURED {}",
                            rendered.unwrap_or("<none>")
                        ),
                    );
                }
                if measured.boundary.degradation != Some(*want_degr) {
                    describe(
                        failures,
                        format!(
                            "EXPECTED degradation {want_degr:?}; MEASURED {:?}",
                            measured.boundary.degradation
                        ),
                    );
                }
                for failure in first_call_cold_clauses(&measured.boundary) {
                    describe(failures, failure);
                }
                for failure in replay_clauses(*warm_replay, &measured.boundary) {
                    describe(failures, failure);
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The matrix cells — pinned measurements
// ─────────────────────────────────────────────────────────────────────────

// Cells are pinned in `matrix_cells.rs`-style inline consts below the
// suite; see `matrix_suite` for the drivers.

#[cfg(test)]
mod matrix_suite {
    use super::matrix::*;
    use super::*;

    fn dump_mode() -> bool {
        std::env::var("U6_CORPUS_DUMP").is_ok_and(|v| v == "1")
    }

    /// THE position cell: binding=let, timing=inside-invoked-IIFE,
    /// depth=1, guard=none, container=none, crossed over every covered
    /// expression position. Checker: every position types `"b"`.
    ///
    /// RECORDED expected-versus-actual GAP (owner: `U6.LOOP_CLOSURE`).
    /// The pinned checker types EVERY position `"b"`; the current tree is
    /// POSITION-DEPENDENT in outcome CLASS: statement / sequence-operand
    /// / call-argument positions REFUSE (typed no-value), while
    /// declarator-init / if-test / template / short-circuit /
    /// object-literal positions never see the write and publish the
    /// STALE pre-write `"a"` CLEAN AND WARM — the wrong-and-warm G4/G5
    /// class. Every per-position outcome is pinned below, so ANY
    /// position-local movement (a fix at one position, a regression at
    /// another) fails loudly and forces a deliberate re-pin — the moment
    /// the owner makes the effect position-independent, this cell is
    /// re-pinned `Uniform` and the live uniformity assertion becomes
    /// unconditional.
    const IIFE_POSITION_CELL: PositionCell = PositionCell {
        id: "let_iife_write_positions",
        binding: BindingKind::Let,
        timing: WriteTiming::InsideInvokedIife,
        depth: 1,
        guard: GuardKind::None,
        container: Container::None,
        positions: COVERED_POSITIONS,
        checker: "\"b\"",
        expectation: CellExpectation::PositionDependent(
            &[
                (
                    ExprPosition::Statement,
                    CellOutcome::NoValue {
                        error: IIFE_EFFECT_REFUSAL,
                    },
                ),
                (
                    ExprPosition::DeclaratorInit,
                    CellOutcome::Value {
                        rendered: "\"a\"",
                        degradation: Degr::None,
                        warm_replay: true,
                    },
                ),
                (
                    ExprPosition::IfTest,
                    CellOutcome::Value {
                        rendered: "\"a\"",
                        degradation: Degr::None,
                        warm_replay: true,
                    },
                ),
                (
                    ExprPosition::SequenceOperand,
                    CellOutcome::NoValue {
                        error: IIFE_EFFECT_REFUSAL,
                    },
                ),
                (
                    ExprPosition::CallArgument,
                    CellOutcome::NoValue {
                        error: IIFE_EFFECT_REFUSAL,
                    },
                ),
                (
                    ExprPosition::Template,
                    CellOutcome::Value {
                        rendered: "\"a\"",
                        degradation: Degr::None,
                        warm_replay: true,
                    },
                ),
                (
                    ExprPosition::ShortCircuit,
                    CellOutcome::Value {
                        rendered: "\"a\"",
                        degradation: Degr::None,
                        warm_replay: true,
                    },
                ),
                (
                    ExprPosition::ObjectLiteral,
                    CellOutcome::Value {
                        rendered: "\"a\"",
                        degradation: Degr::None,
                        warm_replay: true,
                    },
                ),
            ],
            "checker types EVERY position \"b\"; current tree refuses at statement / \
             sequence-operand / call-argument positions and publishes the stale pre-write \
             \"a\" CLEAN AND WARM at the five other positions — the G4/G5 wrong-and-warm \
             class, owner U6.LOOP_CLOSURE",
        ),
    };

    /// The non-position cells: write timing × binding kind × guard ×
    /// completion container, each a single program.
    const FIXED_CELLS: &[FixedCell] = &[
        FixedCell {
            id: "let_write_before_creation",
            binding: BindingKind::Let,
            timing: WriteTiming::BeforeCreation,
            depth: 1,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps() { let x: \"a\" | \"b\" = \"a\"; x = \"b\"; const f = () => x; return f }",
            checker: "() => \"b\"",
            outcome: CellOutcome::Value {
                rendered: "() => \"b\"",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "",
        },
        FixedCell {
            id: "let_write_after_creation",
            binding: BindingKind::Let,
            timing: WriteTiming::AfterCreation,
            depth: 1,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps() { let x: \"a\" | \"b\" = \"a\"; const f = () => x; x = \"b\"; return f }",
            checker: "() => \"a\" | \"b\"",
            outcome: CellOutcome::Value {
                rendered: "() => \"a\"",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "the write AFTER closure creation is not joined into the captured read — the \
                  G6 class, wrong-and-warm; owner U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "let_sibling_closure_write",
            binding: BindingKind::Let,
            timing: WriteTiming::SiblingClosure,
            depth: 1,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps() { let x: \"a\" | \"b\" = \"a\"; const w = () => { x = \"b\" }; void w; return () => x }",
            checker: "() => \"a\" | \"b\"",
            outcome: CellOutcome::Value {
                rendered: "() => \"a\"",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "the SIBLING-closure write never invalidates the captured read — the G7 \
                  class, wrong-and-warm; owner U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "let_deeper_closure_write",
            binding: BindingKind::Let,
            timing: WriteTiming::DeeperClosure,
            depth: 2,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps() { let x: \"a\" | \"b\" = \"a\"; const w = () => () => { x = \"b\" }; void w; return () => x }",
            checker: "() => \"a\" | \"b\"",
            outcome: CellOutcome::Value {
                rendered: "() => \"a\"",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "the DEEPER-closure (depth 2) write never invalidates the captured read — \
                  the G7 class, wrong-and-warm; owner U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "var_write_after_creation",
            binding: BindingKind::Var,
            timing: WriteTiming::AfterCreation,
            depth: 1,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps() { var x: \"a\" | \"b\" = \"a\"; const f = () => x; x = \"b\"; return f }",
            checker: "() => \"a\" | \"b\"",
            outcome: CellOutcome::Value {
                rendered: "() => Union(\"a\" | \"b\")",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "",
        },
        FixedCell {
            id: "param_write_after_creation",
            binding: BindingKind::Param,
            timing: WriteTiming::AfterCreation,
            depth: 1,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps(x: \"a\" | \"b\") { const f = () => x; x = \"b\"; return f }",
            checker: "() => \"a\" | \"b\"",
            outcome: CellOutcome::Value {
                rendered: "() => Union(\"a\" | \"b\")",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "",
        },
        FixedCell {
            id: "const_capture_never_written",
            binding: BindingKind::Const,
            timing: WriteTiming::Never,
            depth: 1,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps() { const x: \"a\" | \"b\" = \"a\"; return () => x }",
            checker: "() => \"a\"",
            outcome: CellOutcome::Value {
                rendered: "() => \"a\"",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "",
        },
        FixedCell {
            id: "typeof_guard_before_creation",
            binding: BindingKind::Param,
            timing: WriteTiming::Never,
            depth: 1,
            guard: GuardKind::TypeofGuard,
            container: Container::If,
            script: "function makeProps(v: string | number) { if (typeof v === \"string\") { return () => v } return () => \"z\" as const }",
            checker: "() => string",
            outcome: CellOutcome::Value {
                rendered: "Union(() => Union(string | number) | () => \"z\")",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "the typeof guard established BEFORE closure creation does not narrow the \
                  captured read (`string | number` where the checker preserves `string`), and \
                  the contributor union is not subtype-collapsed to `() => string` — the G9 \
                  class, wrong-and-warm; owner U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "iife_write_in_try_finally",
            binding: BindingKind::Let,
            timing: WriteTiming::InsideInvokedIife,
            depth: 1,
            guard: GuardKind::None,
            container: Container::TryFinally,
            script: "function sink(v: boolean) { }\nfunction makeProps() { let x: \"a\" | \"b\" = \"a\"; try { (() => { x = \"b\"; return true })(); } finally { } return x }",
            checker: "\"b\"",
            outcome: CellOutcome::NoValue { error: IIFE_EFFECT_REFUSAL },
            gap: "checker types \"b\"; the invoked-IIFE capture-write inside a try/finally \
                  refuses (typed no-value) — an HONEST cold refusal, not wrong-and-warm; \
                  owner U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "iife_write_in_labeled_block",
            binding: BindingKind::Let,
            timing: WriteTiming::InsideInvokedIife,
            depth: 1,
            guard: GuardKind::None,
            container: Container::Labeled,
            script: "function sink(v: boolean) { }\nfunction makeProps() { let x: \"a\" | \"b\" = \"a\"; L: { (() => { x = \"b\"; return true })(); } return x }",
            checker: "\"b\"",
            outcome: CellOutcome::NoValue { error: IIFE_EFFECT_REFUSAL },
            gap: "checker types \"b\"; the invoked-IIFE capture-write inside a labeled block \
                  refuses (typed no-value) — honest cold refusal; owner U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "iife_write_in_if_branch",
            binding: BindingKind::Let,
            timing: WriteTiming::InsideInvokedIife,
            depth: 1,
            guard: GuardKind::None,
            container: Container::If,
            script: "function sink(v: boolean) { }\nfunction makeProps(k: boolean) { let x: \"a\" | \"b\" = \"a\"; if (k) { (() => { x = \"b\"; return true })(); } return x }",
            checker: "\"a\" | \"b\"",
            outcome: CellOutcome::NoValue { error: IIFE_EFFECT_REFUSAL },
            gap: "checker types \"a\" | \"b\"; the conditionally-executed invoked-IIFE \
                  capture-write refuses (typed no-value) — honest cold refusal; owner \
                  U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "iife_write_in_switch_case",
            binding: BindingKind::Let,
            timing: WriteTiming::InsideInvokedIife,
            depth: 1,
            guard: GuardKind::None,
            container: Container::Switch,
            script: "function sink(v: boolean) { }\nfunction makeProps(k: number) { let x: \"a\" | \"b\" = \"a\"; switch (k) { case 1: (() => { x = \"b\"; return true })(); break; default: break } return x }",
            checker: "\"a\" | \"b\"",
            outcome: CellOutcome::NoValue { error: IIFE_EFFECT_REFUSAL },
            gap: "checker types \"a\" | \"b\"; the invoked-IIFE capture-write inside a switch \
                  case refuses (typed no-value) — honest cold refusal; owner U6.LOOP_CLOSURE",
        },
    ];

    /// Dump every cell's measured outcome (`U6_CORPUS_DUMP=1`) so pins
    /// are measured, never guessed.
    #[test]
    fn matrix_cells_hold_their_pins() {
        let dump = dump_mode();
        let mut failures = Vec::new();
        for position in IIFE_POSITION_CELL.positions {
            let script = iife_position_program(*position);
            if dump {
                let m = measure_cell(
                    &format!("{}__{}", IIFE_POSITION_CELL.id, position.id()),
                    &script,
                );
                println!(
                    "MATRIX {} [{}] => rendered {:?}  degr {:?}  warm2 {}  cold2 {}  novalue \
                     {}  novalue2 {}",
                    IIFE_POSITION_CELL.id,
                    position.id(),
                    m.rendered,
                    m.boundary.degradation,
                    m.boundary.second_from_cache,
                    m.boundary.second_cold_computes,
                    m.boundary.error.is_some(),
                    m.boundary.second_error.is_some()
                );
                continue;
            }
            match &IIFE_POSITION_CELL.expectation {
                CellExpectation::Uniform(outcome) => check_cell_outcome(
                    IIFE_POSITION_CELL.id,
                    &IIFE_POSITION_CELL.coords(),
                    Some(*position),
                    &script,
                    IIFE_POSITION_CELL.checker,
                    outcome,
                    &mut failures,
                ),
                CellExpectation::PositionDependent(pins, _note) => {
                    let pinned = pins
                        .iter()
                        .find(|(pos, _)| pos == position)
                        .map(|(_, outcome)| outcome)
                        .unwrap_or_else(|| {
                            panic!(
                                "matrix cell {}: position {} has no pinned outcome",
                                IIFE_POSITION_CELL.id,
                                position.id()
                            )
                        });
                    check_cell_outcome(
                        IIFE_POSITION_CELL.id,
                        &IIFE_POSITION_CELL.coords(),
                        Some(*position),
                        &script,
                        IIFE_POSITION_CELL.checker,
                        pinned,
                        &mut failures,
                    );
                }
            }
        }
        for cell in FIXED_CELLS {
            if dump {
                let m = measure_cell(cell.id, cell.script);
                println!(
                    "MATRIX {} => rendered {:?}  degr {:?}  warm2 {}  cold2 {}  novalue {}  \
                     novalue2 {}",
                    cell.id,
                    m.rendered,
                    m.boundary.degradation,
                    m.boundary.second_from_cache,
                    m.boundary.second_cold_computes,
                    m.boundary.error.is_some(),
                    m.boundary.second_error.is_some()
                );
                continue;
            }
            let coords = format!(
                "{} | gap: {}",
                cell.coords(),
                if cell.gap.is_empty() {
                    "none (pin agrees with the checker)"
                } else {
                    cell.gap
                }
            );
            check_cell_outcome(
                cell.id,
                &coords,
                None,
                cell.script,
                cell.checker,
                &cell.outcome,
                &mut failures,
            );
        }
        if dump {
            panic!(
                "U6_CORPUS_DUMP=1: measurements dumped above; matrix_cells_hold_their_pins \
                 EVALUATED NO PINS in this mode. A dump run is measurement, never evidence — \
                 re-run without U6_CORPUS_DUMP for a verdict."
            );
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// THE key §7 assertion: the same capture-write effect must surface
    /// for a given cell REGARDLESS of expression position, measured LIVE
    /// and independently of the per-position pin comparisons above.
    ///
    /// Two arms, both discriminating:
    ///
    /// * a cell pinned `Uniform` asserts every position's live outcome is
    ///   pairwise IDENTICAL — the unconditional §7 property; once the
    ///   `U6.LOOP_CLOSURE` owner repairs the position dependence and the
    ///   cell is re-pinned `Uniform`, a position-specific effect hook can
    ///   never be reintroduced without failing here;
    /// * a cell pinned `PositionDependent` (today's RECORDED gap) asserts
    ///   the live outcomes are identical WITHIN each pinned-outcome group
    ///   AND still DIVERGE between groups — so a fix landing at one
    ///   position but not its siblings, a regression at one position, or
    ///   the full fix landing without a re-pin, each fails loudly. The
    ///   divergence is asserted from the LIVE tree, so this arm also
    ///   fails if the pins were edited into divergence the tree does not
    ///   actually have.
    #[test]
    fn same_capture_write_cell_is_position_independent() {
        if dump_mode() {
            panic!(
                "U6_CORPUS_DUMP=1: same_capture_write_cell_is_position_independent EVALUATES \
                 NOTHING in this mode (its measurements are the position dump in \
                 matrix_cells_hold_their_pins). A dump run is measurement, never evidence — \
                 re-run without U6_CORPUS_DUMP for a verdict."
            );
        }
        let mut live: Vec<(ExprPosition, String)> = Vec::new();
        for position in IIFE_POSITION_CELL.positions {
            let script = iife_position_program(*position);
            let m = measure_cell(&format!("uniformity__{}", position.id()), &script);
            live.push((
                *position,
                format!(
                    "rendered={:?} degr={:?} warm2={} novalue={} novalue2={} recomputes2={}",
                    m.rendered,
                    m.boundary.degradation,
                    m.boundary.second_from_cache,
                    m.boundary.error.is_some(),
                    m.boundary.second_error.is_some(),
                    m.boundary.second_cold_computes >= 1
                ),
            ));
        }
        let live_of = |position: ExprPosition| -> &String {
            &live
                .iter()
                .find(|(pos, _)| *pos == position)
                .expect("every covered position was measured")
                .1
        };
        match &IIFE_POSITION_CELL.expectation {
            CellExpectation::Uniform(_) => {
                let first = &live[0].1;
                let divergent: Vec<String> = live
                    .iter()
                    .filter(|(_, outcome)| outcome != first)
                    .map(|(pos, outcome)| format!("  {}: {}", pos.id(), outcome))
                    .collect();
                assert!(
                    divergent.is_empty(),
                    "the SAME capture-write (cell {}, {}) measures DIFFERENT outcomes across \
                     expression positions — a position-specific effect hook was reintroduced. \
                     baseline at `{}`: {}\ndivergent positions:\n{}\n[oracle: {}]\n[profile: {}]",
                    IIFE_POSITION_CELL.id,
                    IIFE_POSITION_CELL.coords(),
                    live[0].0.id(),
                    first,
                    divergent.join("\n"),
                    ORACLE_STAMP,
                    PROFILE_STAMP
                );
            }
            CellExpectation::PositionDependent(pins, note) => {
                // (a) WITHIN a pinned-outcome group the live outcomes must
                // be identical — a position-local movement inside a group
                // fails here.
                for (position, pinned) in *pins {
                    let group_baseline = pins
                        .iter()
                        .find(|(_, other)| other == pinned)
                        .expect("the group contains its own member");
                    let baseline_live = live_of(group_baseline.0);
                    assert_eq!(
                        live_of(*position),
                        baseline_live,
                        "cell {}: positions `{}` and `{}` are pinned to the SAME outcome but \
                         measure DIFFERENTLY live — a position-local movement; re-measure and \
                         re-pin (note: {note})\n[oracle: {}]\n[profile: {}]",
                        IIFE_POSITION_CELL.id,
                        position.id(),
                        group_baseline.0.id(),
                        ORACLE_STAMP,
                        PROFILE_STAMP
                    );
                }
                // (b) BETWEEN differently-pinned groups the live outcomes
                // must still diverge — the recorded gap must be REAL in
                // the tree. When the owner repairs position dependence,
                // this fires and forces the re-pin to `Uniform`.
                let (first_position, first_pin) = &pins[0];
                let counter = pins
                    .iter()
                    .find(|(_, pinned)| pinned != first_pin)
                    .expect("position_dependent_pins_record_a_real_divergence guards this");
                assert_ne!(
                    live_of(*first_position),
                    live_of(counter.0),
                    "cell {}: the recorded position-dependence is GONE — `{}` and `{}` now \
                     measure identically. This failure is the INTENDED signal: the owner made \
                     the capture-write effect position-independent; re-pin the cell as \
                     `Uniform` (note: {note})\n[oracle: {}]\n[profile: {}]",
                    IIFE_POSITION_CELL.id,
                    first_position.id(),
                    counter.0.id(),
                    ORACLE_STAMP,
                    PROFILE_STAMP
                );
            }
        }
    }

    /// A refusal cell records WHY it diverges: a pinned `NoValue` outcome
    /// on a cell whose checker names a real type MUST carry a non-empty
    /// `gap` record, and an agreeing cell must not carry a stale one.
    #[test]
    fn no_value_cells_record_their_gap() {
        for cell in FIXED_CELLS {
            if matches!(cell.outcome, CellOutcome::NoValue { .. }) {
                assert!(
                    !cell.gap.is_empty(),
                    "cell {}: a NoValue pin against checker `{}` is a divergence and must \
                     record its gap",
                    cell.id,
                    cell.checker
                );
            }
        }
    }

    /// The `checker` column of an AGREEING fixed cell (empty `gap`) is
    /// BOUND to its pinned outcome: wherever the checker's printed syntax
    /// coincides with the renderer's syntax, the pinned rendering must
    /// EQUAL the checker text verbatim, and the named render-divergent
    /// exceptions must GENUINELY differ (so the exception list cannot
    /// rot into covering an agreement). A cell whose pin diverges from
    /// its checker semantically records that divergence in `gap` (guarded
    /// by [`no_value_cells_record_their_gap`] and the pin comparisons);
    /// this test closes the remaining hole where an AGREEING cell's
    /// `checker` column could be edited freely with no pin noticing.
    #[test]
    fn agreeing_fixed_cells_bind_checker_to_their_pin() {
        /// Agreeing cells whose pinned rendering is NOT the checker's
        /// print syntax: the renderer spells a union `Union(a | b)` where
        /// the checker prints `a | b`. For these the semantic agreement
        /// is held by the live pin comparison, not by text equality.
        const RENDER_DIVERGENT: &[&str] =
            &["var_write_after_creation", "param_write_after_creation"];
        let mut seen_divergent: Vec<&str> = Vec::new();
        for cell in FIXED_CELLS {
            if !cell.gap.is_empty() {
                continue;
            }
            let CellOutcome::Value { rendered, .. } = cell.outcome else {
                panic!(
                    "cell {}: an agreeing (gap-less) cell must pin a value — \
                     no_value_cells_record_their_gap guards the inverse",
                    cell.id
                );
            };
            if RENDER_DIVERGENT.contains(&cell.id) {
                assert_ne!(
                    rendered, cell.checker,
                    "cell {}: listed render-divergent but its pinned rendering EQUALS the \
                     checker — drop it from RENDER_DIVERGENT so the binding is direct",
                    cell.id
                );
                seen_divergent.push(cell.id);
            } else {
                assert_eq!(
                    rendered, cell.checker,
                    "cell {}: an agreeing (gap-less) cell's pinned rendering must EQUAL its \
                     `checker` column verbatim — one of the two was edited without the other. \
                     [oracle: {ORACLE_STAMP}]",
                    cell.id
                );
            }
        }
        assert_eq!(
            seen_divergent.len(),
            RENDER_DIVERGENT.len(),
            "RENDER_DIVERGENT names a cell that is not an agreeing fixed cell: \
             {RENDER_DIVERGENT:?} vs seen {seen_divergent:?}"
        );
    }

    /// A PositionDependent pin must record a REAL divergence; if every
    /// pinned outcome is equal the cell must be re-pinned as `Uniform`.
    #[test]
    fn position_dependent_pins_record_a_real_divergence() {
        let cells: &[&PositionCell] = &[&IIFE_POSITION_CELL];
        for cell in cells {
            if let CellExpectation::PositionDependent(pins, note) = &cell.expectation {
                assert!(
                    pins.len() >= 2,
                    "matrix cell {}: a PositionDependent pin needs at least two positions",
                    cell.id
                );
                let first = &pins[0].1;
                assert!(
                    pins.iter().any(|(_, outcome)| outcome != first),
                    "matrix cell {}: every PositionDependent pin is IDENTICAL — the recorded \
                     gap is not a gap; re-pin the cell as Uniform (note: {note})",
                    cell.id
                );
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Negative controls — the MATRIX COMPARATOR must be able to fail
// ─────────────────────────────────────────────────────────────────────────

/// [`matrix::check_cell_outcome`] is the sole comparison engine behind
/// `matrix_cells_hold_their_pins`. Every clause of it is fed a
/// deliberately WRONG pin against a REAL measurement here, so neutering
/// any comparison clause fails exactly its control instead of leaving
/// the whole matrix suite silently green.
#[cfg(test)]
mod matrix_outcome_controls {
    use super::matrix::*;
    use super::*;

    /// The clean value program every wrong-pin control measures: renders
    /// `() => "a"`, degradation `None`, warm second call (the
    /// `const_capture_never_written` cell's own script, whose truthful
    /// pin `matrix_cells_hold_their_pins` holds).
    const VALUE_SCRIPT: &str =
        "function makeProps() { const x: \"a\" | \"b\" = \"a\"; return () => x }";
    const VALUE_RENDERED: &str = "() => \"a\"";

    fn run(pin: &CellOutcome, id: &str) -> Vec<String> {
        let mut failures = Vec::new();
        check_cell_outcome(
            id,
            "matrix-comparator control",
            None,
            VALUE_SCRIPT,
            "() => \"a\"",
            pin,
            &mut failures,
        );
        failures
    }

    /// CONTROL — the three VALUE-arm clauses each reject a wrong pin, and
    /// reject it ALONE: rendering, typed degradation, warm-replay state.
    /// The truthful pin holds first, so each subsequent failure is
    /// attributable to exactly the one deliberately-wrong column.
    #[test]
    fn cell_value_clauses_each_reject_a_wrong_pin() {
        let truthful = CellOutcome::Value {
            rendered: VALUE_RENDERED,
            degradation: Degr::None,
            warm_replay: true,
        };
        let fails = run(&truthful, "ctl_cell_truthful");
        assert!(
            fails.is_empty(),
            "the truthful pin must hold before any wrong-pin control means anything: {fails:?}"
        );

        let wrong_rendered = CellOutcome::Value {
            rendered: "() => \"b\"",
            degradation: Degr::None,
            warm_replay: true,
        };
        let fails = run(&wrong_rendered, "ctl_cell_wrong_rendered");
        assert!(
            fails.len() == 1 && fails[0].contains("EXPECTED rendering"),
            "a WRONG RENDERING pin must fail exactly the rendering clause — a comparator that \
             accepts `() => \"b\"` for a measured `() => \"a\"` compares nothing: {fails:?}"
        );

        let wrong_degr = CellOutcome::Value {
            rendered: VALUE_RENDERED,
            degradation: Degr::UnmodeledPosition,
            warm_replay: true,
        };
        let fails = run(&wrong_degr, "ctl_cell_wrong_degr");
        assert!(
            fails.len() == 1 && fails[0].contains("EXPECTED degradation"),
            "a WRONG DEGRADATION pin must fail exactly the degradation clause: {fails:?}"
        );

        let wrong_warm = CellOutcome::Value {
            rendered: VALUE_RENDERED,
            degradation: Degr::None,
            warm_replay: false,
        };
        let fails = run(&wrong_warm, "ctl_cell_wrong_warm");
        assert!(
            fails.len() == 2
                && fails
                    .iter()
                    .any(|f| f.contains("must NOT be admitted warm"))
                && fails.iter().any(|f| f.contains("COLD-COMPUTE again")),
            "a warm_replay=false pin against a warm-replaying measurement must fail BOTH \
             cold-replay clauses, each named — warm non-admission AND the missing cold \
             recompute; these are the clauses that catch a ReturnOnly result admitted warm: \
             {fails:?}"
        );
    }

    /// CONTROL — the outcome-CLASS clauses, both directions: a `NoValue`
    /// pin against a real measured VALUE fails, and a `Value` pin against
    /// a real measured REFUSAL fails.
    #[test]
    fn cell_outcome_class_clauses_reject_the_opposite_class() {
        let fails = run(
            &CellOutcome::NoValue {
                error: IIFE_EFFECT_REFUSAL,
            },
            "ctl_cell_novalue_pin",
        );
        assert!(
            fails.len() == 1 && fails[0].contains("EXPECTED a typed no-value refusal"),
            "a NoValue pin against a real measured value must fail the class clause: {fails:?}"
        );

        // The statement-position IIFE-write program REFUSES today (its
        // truthful NoValue pin is held by matrix_cells_hold_their_pins).
        let script = iife_position_program(ExprPosition::Statement);
        let mut failures = Vec::new();
        check_cell_outcome(
            "ctl_cell_value_pin_vs_refusal",
            "matrix-comparator control",
            Some(ExprPosition::Statement),
            &script,
            "\"b\"",
            &CellOutcome::Value {
                rendered: VALUE_RENDERED,
                degradation: Degr::None,
                warm_replay: true,
            },
            &mut failures,
        );
        assert!(
            failures.len() == 1 && failures[0].contains("MEASURED a no-value refusal"),
            "a Value pin against a real measured refusal must fail the class clause and STOP \
             (one failure, not a cascade of unset-field noise): {failures:?}"
        );
    }

    /// CONTROL — the NoValue arm's refusal-replay delegation: a REAL
    /// refusal measurement with ONLY `second_from_cache` substituted
    /// (a cached refusal) must fail the non-admission clause THROUGH
    /// the cell comparator, so neutering the delegation (not only the
    /// underlying clauses) leaves this control red.
    #[test]
    fn cell_no_value_arm_rejects_a_cached_refusal() {
        let script = iife_position_program(ExprPosition::Statement);
        let real = measure_cell("ctl_cell_cached_refusal", &script);
        let r = &real.boundary;
        assert!(
            r.error.is_some() && !r.second_from_cache && r.second_cold_computes >= 1,
            "control precondition: the statement-position program refuses cold on both \
             calls; measured {r:?}"
        );
        let truthful = LaneMeasurement {
            boundary: MeasuredBoundary {
                first_from_cache: r.first_from_cache,
                first_cold_computes: r.first_cold_computes,
                degradation: r.degradation,
                json: r.json.clone(),
                second_from_cache: r.second_from_cache,
                second_cold_computes: r.second_cold_computes,
                second_degradation: r.second_degradation,
                second_json: r.second_json.clone(),
                error: r.error.clone(),
                error_kind: r.error_kind,
                second_error: r.second_error.clone(),
                second_error_kind: r.second_error_kind,
            },
            expect_failures: None,
            rendered: None,
        };
        let mut failures = Vec::new();
        check_cell_outcome_measured(
            "ctl_cell_cached_refusal",
            "matrix-comparator control",
            Some(ExprPosition::Statement),
            &script,
            "\"b\"",
            &CellOutcome::NoValue {
                error: IIFE_EFFECT_REFUSAL,
            },
            &truthful,
            &mut failures,
        );
        assert!(
            failures.is_empty(),
            "the truthful refusal must hold through the cell comparator: {failures:?}"
        );
        let cached = LaneMeasurement {
            boundary: MeasuredBoundary {
                first_from_cache: r.first_from_cache,
                first_cold_computes: r.first_cold_computes,
                degradation: r.degradation,
                json: r.json.clone(),
                second_from_cache: true,
                second_cold_computes: r.second_cold_computes,
                second_degradation: r.second_degradation,
                second_json: r.second_json.clone(),
                error: r.error.clone(),
                error_kind: r.error_kind,
                second_error: r.second_error.clone(),
                second_error_kind: r.second_error_kind,
            },
            expect_failures: None,
            rendered: None,
        };
        let mut failures = Vec::new();
        check_cell_outcome_measured(
            "ctl_cell_cached_refusal",
            "matrix-comparator control",
            Some(ExprPosition::Statement),
            &script,
            "\"b\"",
            &CellOutcome::NoValue {
                error: IIFE_EFFECT_REFUSAL,
            },
            &cached,
            &mut failures,
        );
        assert!(
            failures.len() == 1 && failures[0].contains("NEVER be admitted warm"),
            "a CACHED refusal must fail the non-admission clause through the cell \
             comparator's refusal delegation: {failures:?}"
        );
    }

    /// CONTROL — the Value arm's FIRST-CALL-COLD delegation: a REAL
    /// clean value measurement with ONLY the first call's cache flag /
    /// compute count substituted must fail exactly the corresponding
    /// first-call-cold clause THROUGH the cell comparator, so a matrix
    /// cell pins first-call coldness at parity with a corpus row and
    /// neutering the delegation (not only the shared clauses) leaves
    /// this control red.
    #[test]
    fn cell_value_arm_rejects_a_warm_first_call() {
        let real = measure_cell("ctl_cell_warm_first", VALUE_SCRIPT);
        let r = &real.boundary;
        assert!(
            r.error.is_none() && !r.first_from_cache && r.first_cold_computes >= 1,
            "control precondition: the clean cell program computes cold on call 1; \
             measured {r:?}"
        );
        let truthful_pin = CellOutcome::Value {
            rendered: VALUE_RENDERED,
            degradation: Degr::None,
            warm_replay: true,
        };
        let substituted = |first_from_cache: bool, first_cold_computes: u32| LaneMeasurement {
            boundary: MeasuredBoundary {
                first_from_cache,
                first_cold_computes,
                degradation: r.degradation,
                json: r.json.clone(),
                second_from_cache: r.second_from_cache,
                second_cold_computes: r.second_cold_computes,
                second_degradation: r.second_degradation,
                second_json: r.second_json.clone(),
                error: None,
                error_kind: None,
                second_error: None,
                second_error_kind: None,
            },
            expect_failures: None,
            rendered: real.rendered.clone(),
        };
        let mut failures = Vec::new();
        check_cell_outcome_measured(
            "ctl_cell_warm_first",
            "matrix-comparator control",
            None,
            VALUE_SCRIPT,
            "() => \"a\"",
            &truthful_pin,
            &substituted(true, r.first_cold_computes),
            &mut failures,
        );
        assert!(
            failures.len() == 1 && failures[0].contains("call 1 reported from_cache=true"),
            "a warm first call must fail exactly the first-call-cold clause through the \
             cell comparator: {failures:?}"
        );
        let mut failures = Vec::new();
        check_cell_outcome_measured(
            "ctl_cell_warm_first",
            "matrix-comparator control",
            None,
            VALUE_SCRIPT,
            "() => \"a\"",
            &truthful_pin,
            &substituted(r.first_from_cache, 0),
            &mut failures,
        );
        assert!(
            failures.len() == 1 && failures[0].contains("cold_computes == 0"),
            "a zero-compute first call must fail exactly the first-cold-computes clause \
             through the cell comparator: {failures:?}"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Negative controls — every expectation form must be able to fail
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod expectation_controls {
    use super::*;

    /// Run `f` against the LIVE dispatch + result node of one program's
    /// public-boundary flow return. Panics when the boundary refuses —
    /// every expectation control drives a value-producing program.
    fn with_flow_node<R>(
        script: &str,
        function: &str,
        f: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
    ) -> R {
        with_live_flow_node("", "prog", script, function, |dispatch, node| {
            let node = node
                .unwrap_or_else(|| panic!("control program produced no value\nscript: {script}"));
            f(dispatch, node)
        })
    }

    /// The degraded (`ReturnOnly`, cold-replay) control program — the
    /// D01 shape. Shared by every control that needs a REAL degraded
    /// trace.
    const DEGRADED_SCRIPT: &str = "class Box { readonly tag = \"box\" }\nfunction makeProps() { \
                                   const f = () => new Box(); return { label: \"x\", made: f() } \
                                   }";

    /// CONTROL — exact literal values, BOTH live variants: `"a"` accepts
    /// `"a"` and REJECTS `"b"`, a number, and a primitive-kind widening;
    /// a measured NUMBER literal (`1 as const` renders `1`) accepts its
    /// bit-exact pin and REJECTS a different numeric value, so the
    /// `Lit::Num` equality clause is controlled, not dead vocabulary.
    #[test]
    fn literal_expectation_rejects_a_different_value() {
        with_flow_node(
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    check_node(dispatch, node, &ExpectedNode::Literal(Lit::Str("a"))).is_empty(),
                    "the measured literal must satisfy its own exact pin"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Literal(Lit::Str("b"))).is_empty(),
                    "an expectation that cannot reject a DIFFERENT literal is a stub"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Literal(Lit::Num(1.0))).is_empty(),
                    "a string literal pin must reject a number literal"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Primitive(PrimitiveKind::String)
                    )
                    .is_empty(),
                    "a literal node must NOT satisfy the widened primitive pin — the two are \
                     distinct semantic classes"
                );
            },
        );
        with_flow_node(
            "function makeProps() { return 1 as const }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    check_node(dispatch, node, &ExpectedNode::Literal(Lit::Num(1.0))).is_empty(),
                    "the measured NUMBER literal must satisfy its own bit-exact pin (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Literal(Lit::Num(2.0))).is_empty(),
                    "a number pin must reject a DIFFERENT numeric value — the bit-equality \
                     clause is the comparison under control here"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Literal(Lit::Str("1"))).is_empty(),
                    "a number literal must NOT satisfy a string pin"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Primitive(PrimitiveKind::Number)
                    )
                    .is_empty(),
                    "a number literal must NOT satisfy the widened primitive pin"
                );
            },
        );
    }

    /// CONTROL — signature parameter ARITY and per-parameter TYPES, on a
    /// live parametered signature (`(a: string, b: number) => "a"`).
    /// Every in-tree `Signature` pin is zero-arity (`params: &[]`), so
    /// without this control the arity clause and the per-parameter
    /// `.all(…)` clause would survive neutering with the suite green —
    /// exactly the uncontrolled-clause defect.
    #[test]
    fn signature_params_and_arity_reject_wrong_shapes() {
        with_flow_node(
            "function makeProps() { return (a: string, b: number) => \"a\" as const }",
            "makeProps",
            |dispatch, node| {
                const STR: ExpectedNode = ExpectedNode::Primitive(PrimitiveKind::String);
                const NUM: ExpectedNode = ExpectedNode::Primitive(PrimitiveKind::Number);
                const RET: ExpectedNode = ExpectedNode::Literal(Lit::Str("a"));
                let measured = render_node(dispatch, node, 0);
                assert!(
                    check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Signature {
                            params: &[STR, NUM],
                            ret: &RET
                        }
                    )
                    .is_empty(),
                    "the parametered signature must satisfy its own exact pin (measured \
                     {measured})"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Signature {
                            params: &[STR],
                            ret: &RET
                        }
                    )
                    .is_empty(),
                    "a MISSING parameter (wrong arity) must fail (measured {measured})"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Signature {
                            params: &[STR, NUM, STR],
                            ret: &RET
                        }
                    )
                    .is_empty(),
                    "an EXTRA parameter (wrong arity) must fail"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Signature {
                            params: &[NUM, NUM],
                            ret: &RET
                        }
                    )
                    .is_empty(),
                    "a WRONG parameter TYPE must fail — the per-parameter clause asserts each \
                     position"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Signature {
                            params: &[NUM, STR],
                            ret: &RET
                        }
                    )
                    .is_empty(),
                    "SWAPPED parameter types must fail — parameters are ORDERED"
                );
            },
        );
    }

    /// CONTROL — signatures: `() => "a"` accepts its own pin and REJECTS
    /// the same signature with return `"b"` — exactly the G6/G7
    /// distinction the five repaired rows turn on. This also proves the
    /// checker-derived expectation for a divergent row REJECTS the
    /// parent tree's value.
    #[test]
    fn signature_expectation_rejects_a_different_return() {
        with_flow_node(
            "function makeProps() { let x: \"a\" | \"b\" = \"a\"; return () => x }",
            "makeProps",
            |dispatch, node| {
                let ret_a = ExpectedNode::Signature {
                    params: &[],
                    ret: &ExpectedNode::Literal(Lit::Str("a")),
                };
                let ret_b = ExpectedNode::Signature {
                    params: &[],
                    ret: &ExpectedNode::Literal(Lit::Str("b")),
                };
                let measured = render_node(dispatch, node, 0);
                let a_ok = check_node(dispatch, node, &ret_a).is_empty();
                let b_ok = check_node(dispatch, node, &ret_b).is_empty();
                assert!(
                    a_ok ^ b_ok,
                    "exactly ONE of `() => \"a\"` / `() => \"b\"` may match (measured {measured}) \
                     — a matcher satisfied by both distinguishes nothing"
                );
            },
        );
    }

    /// CONTROL — the signature KIND discriminant, live, in BOTH
    /// directions. The annotation-typed parameter form reaches a
    /// genuine `SignatureKind::Construct` node on the body-derived rail
    /// (`function makeProps(x: new () => Box) { return x }` measures
    /// `new () => DeclRef(Box)`) — the reachability fact a sample-probe
    /// argument misses. A construct signature must be REJECTED where a
    /// call signature is pinned; a call signature must be REJECTED
    /// where a construct signature is pinned; and the checker-syntax
    /// function print (call-only grammar) must reject the construct
    /// node too.
    #[test]
    fn construct_signature_is_distinct_from_call_signature() {
        const BOX_REF: ExpectedNode = ExpectedNode::DeclRef { name: "Box" };
        with_flow_node(
            "class Box { readonly tag = \"box\" }\nfunction makeProps(x: new () => Box) { \
             return x }",
            "makeProps",
            |dispatch, node| {
                let measured = render_node(dispatch, node, 0);
                assert!(
                    check_node(
                        dispatch,
                        node,
                        &ExpectedNode::ConstructSignature {
                            params: &[],
                            ret: &BOX_REF
                        }
                    )
                    .is_empty(),
                    "the live construct signature must satisfy its own construct pin \
                     (measured {measured})"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Signature {
                            params: &[],
                            ret: &BOX_REF
                        }
                    )
                    .is_empty(),
                    "a CONSTRUCT signature must be REJECTED where a CALL signature is pinned \
                     — deleting the discriminant makes this pass silently (measured {measured})"
                );
                let checker_fn = checker_syntax::parse("() => Box").expect("call print parses");
                assert!(
                    !checker_syntax::matches_node(dispatch, node, &checker_fn, 0),
                    "the checker-syntax function print (call-only grammar) must reject the \
                     live construct node (measured {measured})"
                );
            },
        );
        // Vice versa, on a live CALL signature.
        with_flow_node(
            "class Box { readonly tag = \"box\" }\nfunction makeProps(x: () => Box) { return \
             x }",
            "makeProps",
            |dispatch, node| {
                let measured = render_node(dispatch, node, 0);
                assert!(
                    check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Signature {
                            params: &[],
                            ret: &BOX_REF
                        }
                    )
                    .is_empty(),
                    "the live call signature must satisfy its own call pin (measured \
                     {measured})"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::ConstructSignature {
                            params: &[],
                            ret: &BOX_REF
                        }
                    )
                    .is_empty(),
                    "a CALL signature must be REJECTED where a CONSTRUCT signature is pinned \
                     (measured {measured})"
                );
            },
        );
        // The construct arm's OWN arity / parameter-type / return
        // clauses, on a live PARAMETERED construct signature — new
        // vocabulary ships controlled, per the module rule.
        with_flow_node(
            "class Box { readonly tag = \"box\" }\nfunction makeProps(x: new (a: string) => \
             Box) { return x }",
            "makeProps",
            |dispatch, node| {
                const STR: ExpectedNode = ExpectedNode::Primitive(PrimitiveKind::String);
                const NUM: ExpectedNode = ExpectedNode::Primitive(PrimitiveKind::Number);
                let measured = render_node(dispatch, node, 0);
                assert!(
                    check_node(
                        dispatch,
                        node,
                        &ExpectedNode::ConstructSignature {
                            params: &[STR],
                            ret: &BOX_REF
                        }
                    )
                    .is_empty(),
                    "the parametered construct signature must satisfy its own exact pin \
                     (measured {measured})"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::ConstructSignature {
                            params: &[],
                            ret: &BOX_REF
                        }
                    )
                    .is_empty(),
                    "a WRONG construct ARITY must fail (measured {measured})"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::ConstructSignature {
                            params: &[NUM],
                            ret: &BOX_REF
                        }
                    )
                    .is_empty(),
                    "a WRONG construct parameter TYPE must fail (measured {measured})"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::ConstructSignature {
                            params: &[STR],
                            ret: &STR
                        }
                    )
                    .is_empty(),
                    "a WRONG construct RETURN must fail (measured {measured})"
                );
            },
        );
    }

    /// CONTROL — the fail-closed STRUCTURAL guards of [`node_matches`]:
    /// an exhausted recursion depth and an absent (evicted / unknown)
    /// node id must NEVER read as a match, exercised DIRECTLY so the
    /// guards are discriminable today rather than a latent hatch the
    /// moment a deep or evicted node lands.
    #[test]
    fn depth_and_absent_node_guards_fail_closed() {
        with_flow_node(
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            |dispatch, node| {
                const PIN: ExpectedNode = ExpectedNode::Literal(Lit::Str("a"));
                assert!(
                    node_matches(dispatch, node, &PIN, 0),
                    "precondition: the pin matches its own node at depth 0"
                );
                assert!(
                    node_matches(dispatch, node, &PIN, MATCH_DEPTH_LIMIT),
                    "precondition: the pin still matches AT the depth limit"
                );
                assert!(
                    !node_matches(dispatch, node, &PIN, MATCH_DEPTH_LIMIT + 1),
                    "an EXHAUSTED depth must fail CLOSED — a matcher that reads past its \
                     recursion bound as a match is a hatch, not a bound"
                );
                let absent = SemanticNodeId(u64::MAX);
                assert!(
                    dispatch.graph().node_data(absent).is_none(),
                    "precondition: the fabricated id resolves to NO node"
                );
                assert!(
                    !node_matches(dispatch, absent, &PIN, 0),
                    "an ABSENT node must fail CLOSED against a literal pin"
                );
                assert!(
                    !node_matches(
                        dispatch,
                        absent,
                        &ExpectedNode::Primitive(PrimitiveKind::String),
                        0
                    ),
                    "an ABSENT node must fail CLOSED against a primitive pin too — the \
                     early-out precedes arm dispatch"
                );
            },
        );
    }

    /// CONTROL — the STRUCTURAL clauses of
    /// [`checker_syntax::matches_node`] that no corpus row reaches:
    /// union length and constituent INJECTIVITY, object length and
    /// member INJECTIVITY, intersection length, the cross-variant
    /// `_ => false` catchall, the depth guard, the absent-node
    /// early-out, and the deleted `Ref` ↔ `BareRef` / `TypeParam`
    /// acceptance arms staying fail-closed. Each clause is exercised on
    /// a LIVE measured node so neutering it leaves this control red.
    #[test]
    fn checker_syntax_structural_clauses_fail_closed() {
        let accepts =
            |dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId, text: &str| -> bool {
                let parsed = checker_syntax::parse(text)
                    .unwrap_or_else(|err| panic!("`{text}` must parse: {err}"));
                checker_syntax::matches_node(dispatch, node, &parsed, 0)
            };
        // Union LENGTH (the subset direction the exact-set rule owns):
        // a three-constituent live union rejects a two-constituent
        // print. With the length clause neutered the two expected
        // constituents each claim a distinct measured one and the
        // subset is (wrongly) accepted.
        with_flow_node(
            "function makeProps(k: number) { if (k === 1) { return \"a\" as const } if (k \
             === 2) { return \"b\" as const } return \"c\" as const }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    accepts(dispatch, node, "\"a\" | \"b\" | \"c\""),
                    "precondition: the three-constituent union accepts its own print \
                     (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !accepts(dispatch, node, "\"a\" | \"b\""),
                    "a SUBSET print must be rejected — union comparison is exact set \
                     equality, and the length clause is what rejects a subset"
                );
            },
        );
        // Union constituent INJECTIVITY, the catchall, the depth guard,
        // the absent-node early-out, all on the two-constituent union.
        with_flow_node(
            "function makeProps(k: boolean) { if (k) { return \"a\" as const } return \"b\" \
             as const }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    accepts(dispatch, node, "\"a\" | \"b\""),
                    "precondition: the union accepts its own print (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !accepts(dispatch, node, "\"a\" | \"a\""),
                    "DUPLICATE print constituents must claim DISTINCT measured constituents \
                     — the `!used[slot]` injectivity clause"
                );
                assert!(
                    !accepts(dispatch, node, "\"a\""),
                    "a scalar print against a measured UNION is a cross-variant pair and \
                     must reach the `_ => false` catchall"
                );
                let parsed = checker_syntax::parse("\"a\" | \"b\"").expect("parses");
                assert!(
                    !checker_syntax::matches_node(dispatch, node, &parsed, MATCH_DEPTH_LIMIT + 1),
                    "an EXHAUSTED depth must fail CLOSED in the checker matcher too"
                );
                let absent = SemanticNodeId(u64::MAX);
                assert!(
                    dispatch.graph().node_data(absent).is_none(),
                    "precondition: the fabricated id resolves to NO node"
                );
                assert!(
                    !checker_syntax::matches_node(dispatch, absent, &parsed, 0),
                    "an ABSENT node must fail CLOSED in the checker matcher too"
                );
            },
        );
        // Object LENGTH (subset direction) and member INJECTIVITY, plus
        // a cross-variant catchall pair, on a live two-member object.
        with_flow_node(
            "function makeProps() { return { label: \"x\", n: 1 } }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    accepts(dispatch, node, "{ label: string; n: number; }"),
                    "precondition: the object accepts its own print (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !accepts(dispatch, node, "{ label: string; }"),
                    "a DROPPED member must be rejected — the object length clause"
                );
                assert!(
                    !accepts(dispatch, node, "{ label: string; label: string; }"),
                    "DUPLICATE print members must claim DISTINCT measured members — the \
                     `used[slot]` injectivity clause"
                );
                assert!(
                    !accepts(dispatch, node, "string"),
                    "a primitive print against a measured OBJECT must reach the catchall"
                );
            },
        );
        // Intersection LENGTH: a three-arm print against the live
        // two-arm intersection. The ordered `zip` TRUNCATES to the
        // shorter side, so with the length clause neutered the extra
        // arm is silently ignored and the print is (wrongly) accepted.
        with_flow_node(
            "type A = { a: number }; type B = { b: number }\nfunction makeProps(x: A & B) { \
             return x }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    accepts(dispatch, node, "A & B"),
                    "precondition: the intersection accepts its own print (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !accepts(dispatch, node, "A & B & B"),
                    "an EXTRA arm must be rejected — the intersection length clause is what \
                     stops the zip truncation"
                );
            },
        );
        // The DELETED `Ref` ↔ `BareRef` / `TypeParam` acceptance arms
        // stay fail-closed: a name print against either node kind now
        // reaches the catchall and matches NOTHING. Reintroducing an
        // arm fails these assertions and forces a control with it.
        with_flow_node(
            "function makeProps(x: NotDeclaredAnywhere) { return x }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    !accepts(dispatch, node, "NotDeclaredAnywhere"),
                    "a name print must NOT match a measured BareRef — the acceptance arm \
                     was deleted as unexercised; reintroduce it only with a row + control \
                     (measured {})",
                    render_node(dispatch, node, 0)
                );
            },
        );
        with_flow_node(
            "function makeProps<T>(x: T) { return x }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    !accepts(dispatch, node, "T"),
                    "a name print must NOT match a measured TypeParam — the acceptance arm \
                     was deleted as unexercised; reintroduce it only with a row + control \
                     (measured {})",
                    render_node(dispatch, node, 0)
                );
            },
        );
    }

    /// CONTROL — union set equality: `"a" | "b"` accepts its exact set in
    /// EITHER order and REJECTS a subset, a superset, and a swapped
    /// member.
    #[test]
    fn union_set_equality_rejects_subset_and_superset() {
        with_flow_node(
            "function makeProps(k: boolean) { if (k) { return \"a\" as const } return \"b\" as \
             const }",
            "makeProps",
            |dispatch, node| {
                const A: ExpectedNode = ExpectedNode::Literal(Lit::Str("a"));
                const B: ExpectedNode = ExpectedNode::Literal(Lit::Str("b"));
                const C: ExpectedNode = ExpectedNode::Literal(Lit::Str("c"));
                assert!(
                    check_node(dispatch, node, &ExpectedNode::Union(&[A, B])).is_empty(),
                    "the exact constituent set must match (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    check_node(dispatch, node, &ExpectedNode::Union(&[B, A])).is_empty(),
                    "union matching must be ORDER-INSENSITIVE"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Union(&[A])).is_empty(),
                    "a SUBSET must fail — set equality, not containment"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Union(&[A, B, C])).is_empty(),
                    "a SUPERSET must fail — set equality, not containment"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Union(&[A, C])).is_empty(),
                    "a swapped member must fail"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Union(&[A, A])).is_empty(),
                    "a duplicate expectation must claim DISTINCT constituents"
                );
            },
        );
    }

    /// CONTROL — intersections: `A & B` accepts its exact arms and
    /// REJECTS a wrong arm.
    #[test]
    fn intersection_expectation_rejects_a_wrong_arm() {
        with_flow_node(
            "type A = { a: number }; type B = { b: number }\nfunction makeProps(x: A & B) { \
             return x }",
            "makeProps",
            |dispatch, node| {
                let measured = render_node(dispatch, node, 0);
                // The measured arms carry the RESOLVED `DeclRef` identity
                // (`Intersection(DeclRef(A) & DeclRef(B))`), pinned HARD
                // here so a graph-identity change is loud. Every negative
                // below exercises that measured identity — no
                // never-executed arm shape rides along.
                const DECL_PAIR: [ExpectedNode; 2] = [
                    ExpectedNode::DeclRef { name: "A" },
                    ExpectedNode::DeclRef { name: "B" },
                ];
                const WRONG_DECL: [ExpectedNode; 2] = [
                    ExpectedNode::DeclRef { name: "A" },
                    ExpectedNode::DeclRef { name: "C" },
                ];
                const REVERSED_DECL: [ExpectedNode; 2] = [
                    ExpectedNode::DeclRef { name: "B" },
                    ExpectedNode::DeclRef { name: "A" },
                ];
                const ONE_DECL: [ExpectedNode; 1] = [ExpectedNode::DeclRef { name: "A" }];
                // Same names, WRONG reference identity: reachable against
                // the measured `DeclRef` arms, and must be rejected — the
                // arm matcher preserves the reference-identity trio inside
                // an intersection.
                const SAME_NAME_BARE: [ExpectedNode; 2] = [
                    ExpectedNode::BareRef { name: "A" },
                    ExpectedNode::BareRef { name: "B" },
                ];
                assert!(
                    check_node(dispatch, node, &ExpectedNode::Intersection(&DECL_PAIR)).is_empty(),
                    "the intersection's own DeclRef arms must match (measured {measured})"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Intersection(&WRONG_DECL))
                        .is_empty(),
                    "an intersection pin that cannot reject a WRONG arm NAME is a stub \
                     (measured {measured})"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Intersection(&REVERSED_DECL))
                        .is_empty(),
                    "REVERSED (but name-correct) arms must fail — intersection arms are matched \
                     in SOURCE ORDER, so a set-equal neutering that accepts `B & A` for a \
                     measured `A & B` leaves this control red (measured {measured})"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Intersection(&ONE_DECL)).is_empty(),
                    "a missing arm must fail"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Intersection(&SAME_NAME_BARE))
                        .is_empty(),
                    "same-name BareRef arms must be rejected against measured DeclRef arms — \
                     reference identity survives inside an intersection (measured {measured})"
                );
            },
        );
    }

    /// CONTROL — graph-level identity: the trio the `TypeExpr` projection
    /// conflates to `Ref {{ name }}` stays distinct here. The measured
    /// reference node accepts EXACTLY ONE of the three same-name pins.
    #[test]
    fn reference_identity_trio_is_distinct() {
        with_flow_node(
            "type Foo = { a: number }\nfunction makeProps(x: Foo) { return x }",
            "makeProps",
            |dispatch, node| {
                let pins = [
                    ("TypeParam", ExpectedNode::TypeParam { name: "Foo" }),
                    ("DeclRef", ExpectedNode::DeclRef { name: "Foo" }),
                    ("BareRef", ExpectedNode::BareRef { name: "Foo" }),
                ];
                let matching: Vec<&str> = pins
                    .iter()
                    .filter(|(_, pin)| check_node(dispatch, node, pin).is_empty())
                    .map(|(label, _)| *label)
                    .collect();
                assert_eq!(
                    matching.len(),
                    1,
                    "EXACTLY one of TypeParam/DeclRef/BareRef may match the measured reference \
                     node (measured {}, matched {matching:?}) — the projection conflates all \
                     three to `Ref {{ name }}`; the graph-level expectation must not",
                    render_node(dispatch, node, 0)
                );
            },
        );
    }

    /// CONTROL — the type-parameter arm of the trio, from a generic
    /// signature.
    #[test]
    fn type_param_identity_is_distinct_from_references() {
        with_flow_node(
            "function makeProps<T>(x: T) { return x }",
            "makeProps",
            |dispatch, node| {
                let type_param =
                    check_node(dispatch, node, &ExpectedNode::TypeParam { name: "T" }).is_empty();
                let decl_ref =
                    check_node(dispatch, node, &ExpectedNode::DeclRef { name: "T" }).is_empty();
                let bare_ref =
                    check_node(dispatch, node, &ExpectedNode::BareRef { name: "T" }).is_empty();
                assert!(
                    usize::from(type_param) + usize::from(decl_ref) + usize::from(bare_ref) <= 1,
                    "at most one identity pin may match (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    type_param || decl_ref || bare_ref,
                    "the generic return must be pinnable as one of the trio (measured {})",
                    render_node(dispatch, node, 0)
                );
            },
        );
    }

    /// CONTROL — cache replay, both directions, each failure named to
    /// its exact clause. A clean result REPLAYS WARM: pinning
    /// `warm_replay: false` against it must fail BOTH cold-arm clauses
    /// (warm non-admission AND the missing cold recompute). A degraded
    /// (`ReturnOnly`) result must NOT be admitted warm: pinning
    /// `warm_replay: true` against it must fail BOTH warm-arm clauses.
    /// The wrong-JSON and wrong-degradation pins each fail EXACTLY their
    /// own clause — this test is the sole control for those two pin-side
    /// clauses, so it names them instead of asserting "some failure
    /// occurred".
    #[test]
    fn cache_replay_assertion_fails_in_both_directions() {
        // Clean program: warm replay is the truth.
        let clean = drive_expect_boundary(
            "",
            "ctl_clean",
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            None,
        );
        let clean_json = clean
            .boundary
            .json
            .clone()
            .expect("clean control projects JSON");
        assert!(
            check_boundary(&clean_json, Degr::None, true, &clean.boundary).is_empty(),
            "the clean control must satisfy its own truthful pin; measured {:?}",
            clean.boundary
        );
        let fails = check_boundary(&clean_json, Degr::None, false, &clean.boundary);
        assert!(
            fails.len() == 2
                && fails
                    .iter()
                    .any(|f| f.contains("must NOT be admitted warm"))
                && fails.iter().any(|f| f.contains("COLD-COMPUTE again")),
            "pinning warm_replay=false against a warm-replaying result must fail BOTH \
             cold-arm clauses, each named — warm non-admission and the missing cold \
             recompute: {fails:?}"
        );
        let fails = check_boundary("{\"wrong\":true}", Degr::None, true, &clean.boundary);
        assert!(
            fails.len() == 1 && fails[0].contains("projected JSON of call 1"),
            "a wrong exact-JSON pin must fail EXACTLY the call-1 projection clause: {fails:?}"
        );
        let fails = check_boundary(&clean_json, Degr::UnmodeledPosition, true, &clean.boundary);
        assert!(
            fails.len() == 1 && fails[0].contains("typed degradation: expected"),
            "a wrong degradation pin must fail EXACTLY the typed-degradation clause: {fails:?}"
        );

        // Degraded program (the D01 shape): ReturnOnly, never warm.
        let degraded =
            drive_expect_boundary("", "ctl_degraded", DEGRADED_SCRIPT, "makeProps", None);
        let degraded_json = degraded
            .boundary
            .json
            .clone()
            .expect("degraded control still projects JSON");
        assert!(
            check_boundary(
                &degraded_json,
                Degr::UnmodeledPosition,
                false,
                &degraded.boundary
            )
            .is_empty(),
            "the degraded control must satisfy its truthful pin (ReturnOnly ⇒ cold replay); \
             measured {:?}",
            degraded.boundary
        );
        let fails = check_boundary(
            &degraded_json,
            Degr::UnmodeledPosition,
            true,
            &degraded.boundary,
        );
        assert!(
            fails.len() == 2
                && fails.iter().any(|f| f.contains("must REPLAY WARM"))
                && fails
                    .iter()
                    .any(|f| f.contains("a warm family replay runs ZERO")),
            "pinning warm_replay=true against a ReturnOnly result must fail BOTH warm-arm \
             clauses, each named — this is the pin that catches a result admitted warm when \
             it should not be: {fails:?}"
        );
    }

    /// CONTROL — the cold arm demands ACTUAL cold work: the REAL
    /// degraded trace with ONLY the second call's compute count zeroed
    /// (a trace that "was not admitted warm" but did nothing either)
    /// must fail exactly the cold-recompute clause.
    #[test]
    fn boundary_cold_replay_requires_actual_cold_work() {
        let degraded =
            drive_expect_boundary("", "ctl_zero_cold", DEGRADED_SCRIPT, "makeProps", None);
        let d = &degraded.boundary;
        assert!(
            d.error.is_none() && !d.second_from_cache && d.second_cold_computes >= 1,
            "control precondition: the degraded program replays COLD with real work; measured \
             {d:?}"
        );
        let degraded_json = d.json.clone().expect("degraded control projects JSON");
        let flagless = MeasuredBoundary {
            first_from_cache: d.first_from_cache,
            first_cold_computes: d.first_cold_computes,
            degradation: d.degradation,
            json: d.json.clone(),
            second_from_cache: d.second_from_cache,
            second_cold_computes: 0,
            second_degradation: d.second_degradation,
            second_json: d.second_json.clone(),
            error: None,
            error_kind: None,
            second_error: None,
            second_error_kind: None,
        };
        let fails = check_boundary(&degraded_json, Degr::UnmodeledPosition, false, &flagless);
        assert!(
            fails.len() == 1 && fails[0].contains("COLD-COMPUTE again"),
            "warm_replay=false with from_cache=false but ZERO cold computes must fail exactly \
             the cold-recompute clause — `second_from_cache == false` alone is NOT cold \
             replay: {fails:?}"
        );
    }

    /// CONTROL — replay result-CLASS drift and typed-DEGRADATION drift,
    /// each failing exactly its clause. Both traces are real
    /// measurements with a single field substituted by another MEASURED
    /// value, stated inline.
    #[test]
    fn boundary_replay_class_and_degradation_drift_fail() {
        // (a) CLASS drift: the real clean trace, with call 2 replaced by
        // a real measured refusal (the statement-position IIFE-write
        // program's second call).
        let clean = drive_expect_boundary(
            "",
            "ctl_class_a",
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            None,
        );
        let refused_script = matrix::iife_position_program(matrix::ExprPosition::Statement);
        let refused = drive_expect_boundary("", "ctl_class_b", &refused_script, "makeProps", None);
        assert!(
            clean.boundary.error.is_none() && refused.boundary.second_error.is_some(),
            "control preconditions: clean produces a value, the refusal refuses on call 2; \
             measured clean {:?} refused {:?}",
            clean.boundary,
            refused.boundary
        );
        let c = &clean.boundary;
        let r = &refused.boundary;
        let clean_json = c.json.clone().expect("clean control projects JSON");
        let class_drifted = MeasuredBoundary {
            first_from_cache: c.first_from_cache,
            first_cold_computes: c.first_cold_computes,
            degradation: c.degradation,
            json: c.json.clone(),
            second_from_cache: r.second_from_cache,
            second_cold_computes: r.second_cold_computes,
            second_degradation: r.second_degradation,
            second_json: r.second_json.clone(),
            error: None,
            error_kind: None,
            second_error: r.second_error.clone(),
            second_error_kind: r.second_error_kind,
        };
        let fails = check_boundary(&clean_json, Degr::None, true, &class_drifted);
        assert!(
            fails.len() == 1 && fails[0].contains("changed the result CLASS"),
            "a second call that refuses where the first produced a value must fail exactly \
             the class-drift clause (and STOP, no unset-field cascade): {fails:?}"
        );

        // (b) DEGRADATION drift: the real degraded trace, with ONLY the
        // second call's typed degradation substituted by the clean
        // trace's measured second-call degradation.
        let degraded =
            drive_expect_boundary("", "ctl_degr_drift", DEGRADED_SCRIPT, "makeProps", None);
        let d = &degraded.boundary;
        assert!(
            d.error.is_none() && d.second_degradation == d.degradation,
            "control precondition: the degraded replay carries the same typed degradation; \
             measured {d:?}"
        );
        let degraded_json = d.json.clone().expect("degraded control projects JSON");
        let degr_drifted = MeasuredBoundary {
            first_from_cache: d.first_from_cache,
            first_cold_computes: d.first_cold_computes,
            degradation: d.degradation,
            json: d.json.clone(),
            second_from_cache: d.second_from_cache,
            second_cold_computes: d.second_cold_computes,
            second_degradation: c.second_degradation,
            second_json: d.second_json.clone(),
            error: None,
            error_kind: None,
            second_error: None,
            second_error_kind: None,
        };
        let fails = check_boundary(
            &degraded_json,
            Degr::UnmodeledPosition,
            false,
            &degr_drifted,
        );
        assert!(
            fails.len() == 1 && fails[0].contains("degradation drifted across the replay"),
            "a replay whose typed degradation drifts from call 1 must fail exactly the \
             degradation-drift clause: {fails:?}"
        );
    }

    /// CONTROL — every [`check_boundary_refusal`] clause, each failing
    /// individually against a single-field-substituted REAL refusal
    /// trace: the truthful pin holds; a CACHED refusal fails the
    /// non-admission clause; a zero-recompute refusal fails the
    /// recompute clause; a second call that produces a VALUE fails the
    /// class clause; the refusal pin against a real VALUE trace fails
    /// the first-call class clause; a WRONG pinned refusal KIND fails
    /// the kind clause; and a second call refusing with a DIFFERENT
    /// measured kind (a real loop-refusal's kind substituted in) fails
    /// the identity-drift clause.
    #[test]
    fn refusal_comparator_clauses_fail_individually() {
        let refused_script = matrix::iife_position_program(matrix::ExprPosition::Statement);
        let refused = drive_expect_boundary("", "ctl_refusal", &refused_script, "makeProps", None);
        let r = &refused.boundary;
        assert!(
            r.error.is_some()
                && r.second_error.is_some()
                && !r.first_from_cache
                && r.first_cold_computes >= 1
                && !r.second_from_cache
                && r.second_cold_computes >= 1,
            "control precondition: the statement-position IIFE-write program refuses on both \
             calls, cold each time; measured {r:?}"
        );
        assert!(
            check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, r).is_empty(),
            "the real refusal must satisfy its own truthful pin; measured {r:?}"
        );

        // (a) A CACHED refusal — only `second_from_cache` substituted.
        let cached = MeasuredBoundary {
            first_from_cache: r.first_from_cache,
            first_cold_computes: r.first_cold_computes,
            degradation: r.degradation,
            json: r.json.clone(),
            second_from_cache: true,
            second_cold_computes: r.second_cold_computes,
            second_degradation: r.second_degradation,
            second_json: r.second_json.clone(),
            error: r.error.clone(),
            error_kind: r.error_kind,
            second_error: r.second_error.clone(),
            second_error_kind: r.second_error_kind,
        };
        let fails = check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, &cached);
        assert!(
            fails.len() == 1 && fails[0].contains("NEVER be admitted warm"),
            "a warm-served refusal must fail exactly the non-admission clause — the cached \
             refusal is the typed-non-admission violation: {fails:?}"
        );

        // (b) A refusal that did no work — only the second compute count
        // zeroed.
        let inert = MeasuredBoundary {
            first_from_cache: r.first_from_cache,
            first_cold_computes: r.first_cold_computes,
            degradation: r.degradation,
            json: r.json.clone(),
            second_from_cache: r.second_from_cache,
            second_cold_computes: 0,
            second_degradation: r.second_degradation,
            second_json: r.second_json.clone(),
            error: r.error.clone(),
            error_kind: r.error_kind,
            second_error: r.second_error.clone(),
            second_error_kind: r.second_error_kind,
        };
        let fails = check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, &inert);
        assert!(
            fails.len() == 1 && fails[0].contains("RECOMPUTE on every demand"),
            "a refusal replay with zero cold computes must fail exactly the recompute \
             clause: {fails:?}"
        );

        // (c) CLASS drift on call 2 — the refusal's second call replaced
        // by the clean program's real measured second-call value.
        let clean = drive_expect_boundary(
            "",
            "ctl_refusal_clean",
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            None,
        );
        let cl = &clean.boundary;
        let value_second = MeasuredBoundary {
            first_from_cache: r.first_from_cache,
            first_cold_computes: r.first_cold_computes,
            degradation: r.degradation,
            json: r.json.clone(),
            second_from_cache: r.second_from_cache,
            second_cold_computes: r.second_cold_computes,
            second_degradation: cl.second_degradation,
            second_json: cl.second_json.clone(),
            error: r.error.clone(),
            error_kind: r.error_kind,
            second_error: None,
            second_error_kind: None,
        };
        let fails = check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, &value_second);
        assert!(
            fails.len() == 1 && fails[0].contains("changed the result CLASS"),
            "a second call that produces a value where call 1 refused must fail exactly the \
             class-drift clause: {fails:?}"
        );

        // (d) The refusal pin against a real VALUE trace.
        let fails = check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, cl);
        assert!(
            fails.len() == 1 && fails[0].contains("produced a VALUE"),
            "the refusal pin against a value-producing trace must fail exactly the \
             first-call class clause (and STOP): {fails:?}"
        );

        // (e) A WARM first refusal — only `first_from_cache` substituted.
        let warm_first = MeasuredBoundary {
            first_from_cache: true,
            first_cold_computes: r.first_cold_computes,
            degradation: r.degradation,
            json: r.json.clone(),
            second_from_cache: r.second_from_cache,
            second_cold_computes: r.second_cold_computes,
            second_degradation: r.second_degradation,
            second_json: r.second_json.clone(),
            error: r.error.clone(),
            error_kind: r.error_kind,
            second_error: r.second_error.clone(),
            second_error_kind: r.second_error_kind,
        };
        let fails = check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, &warm_first);
        assert!(
            fails.len() == 1 && fails[0].contains("a refusal on a fresh host must be COLD"),
            "a warm first refusal must fail exactly the first-call-cold clause: {fails:?}"
        );

        // (f) A first refusal with no cold work — only the first compute
        // count zeroed.
        let conjured = MeasuredBoundary {
            first_from_cache: r.first_from_cache,
            first_cold_computes: 0,
            degradation: r.degradation,
            json: r.json.clone(),
            second_from_cache: r.second_from_cache,
            second_cold_computes: r.second_cold_computes,
            second_degradation: r.second_degradation,
            second_json: r.second_json.clone(),
            error: r.error.clone(),
            error_kind: r.error_kind,
            second_error: r.second_error.clone(),
            second_error_kind: r.second_error_kind,
        };
        let fails = check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, &conjured);
        assert!(
            fails.len() == 1 && fails[0].contains("computed, not conjured"),
            "a zero-compute first refusal must fail exactly the first-cold-computes \
             clause: {fails:?}"
        );

        // (g) A WRONG pinned refusal KIND against the untouched real
        // trace: the pin names a DIFFERENT typed refusal (a real
        // vocabulary member, `Failure(Missing)`) than the one measured.
        // Only the kind clause may fire.
        let fails = check_boundary_refusal(FlowReturnError::Failure(FlowReturnFailure::Missing), r);
        assert!(
            fails.len() == 1 && fails[0].contains("typed refusal KIND"),
            "a refusal swapped for a different typed refusal must fail exactly the kind \
             clause — `is_some()` alone would wave it through: {fails:?}"
        );

        // (h) Refusal IDENTITY drift across the replay: the real trace
        // with ONLY the second call's typed kind substituted by a
        // DIFFERENT program's real measured refusal kind (a
        // return-bearing loop, which refuses with a distinct typed
        // kind). Only the identity-drift clause may fire.
        let loop_refused = drive_expect_boundary(
            "",
            "ctl_refusal_loop",
            "function makeProps() { while (true) { return \"a\" as const } }",
            "makeProps",
            None,
        );
        let lr = &loop_refused.boundary;
        assert!(
            lr.error_kind.is_some() && lr.error_kind != r.error_kind,
            "control precondition: the return-bearing loop refuses with a DIFFERENT typed \
             kind than the IIFE-write program; measured loop {:?} vs iife {:?}",
            lr.error_kind,
            r.error_kind
        );
        let kind_drifted = MeasuredBoundary {
            first_from_cache: r.first_from_cache,
            first_cold_computes: r.first_cold_computes,
            degradation: r.degradation,
            json: r.json.clone(),
            second_from_cache: r.second_from_cache,
            second_cold_computes: r.second_cold_computes,
            second_degradation: r.second_degradation,
            second_json: r.second_json.clone(),
            error: r.error.clone(),
            error_kind: r.error_kind,
            second_error: lr.error.clone(),
            second_error_kind: lr.error_kind,
        };
        let fails = check_boundary_refusal(matrix::IIFE_EFFECT_REFUSAL, &kind_drifted);
        assert!(
            fails.len() == 1 && fails[0].contains("refusal IDENTITY drifted"),
            "a refusal that changes kind across the two calls must fail exactly the \
             identity-drift clause: {fails:?}"
        );
    }

    /// CONTROL — object members: the exact member set matches; a wrong
    /// member VALUE, a wrong member NAME, a MISSING member, an EXTRA
    /// member, and DUPLICATE expected keys each reject. The duplicate-key
    /// rejection is the INJECTIVITY control: two expected `label` entries
    /// must not both be satisfied by the single measured `label` member.
    #[test]
    fn object_expectation_rejects_wrong_missing_extra_and_duplicate_members() {
        with_flow_node(
            "function makeProps() { return { label: \"x\", n: 1 } }",
            "makeProps",
            |dispatch, node| {
                const STR: ExpectedNode = ExpectedNode::Primitive(PrimitiveKind::String);
                const NUM: ExpectedNode = ExpectedNode::Primitive(PrimitiveKind::Number);
                assert!(
                    check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Object(&[("label", STR), ("n", NUM)])
                    )
                    .is_empty(),
                    "the exact member set must match (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Object(&[("label", NUM), ("n", NUM)])
                    )
                    .is_empty(),
                    "a WRONG MEMBER VALUE must fail — an object pin that cannot reject a wrong \
                     member value asserts member names only"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Object(&[("wrong", STR), ("n", NUM)])
                    )
                    .is_empty(),
                    "a WRONG MEMBER NAME must fail"
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::Object(&[("label", STR)]))
                        .is_empty(),
                    "a MISSING member must fail — set equality, not containment"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Object(&[("label", STR), ("n", NUM), ("zzz", STR)])
                    )
                    .is_empty(),
                    "an EXTRA member must fail — set equality, not containment"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Object(&[("label", STR), ("label", STR)])
                    )
                    .is_empty(),
                    "DUPLICATE expected keys must claim DISTINCT measured members — \
                     non-injective matching would satisfy both against the one `label` member"
                );
            },
        );
    }

    /// CONTROL — the typed unmodelled-position marker: the D01 shape's
    /// `made` member measures `Opaque(UnmodeledPosition)`; the pin matches
    /// it there and REJECTS a modelled member, so the variant is
    /// exercised and discriminating, not a dead vocabulary row.
    #[test]
    fn opaque_unmodeled_position_marker_is_discriminating() {
        with_flow_node(DEGRADED_SCRIPT, "makeProps", |dispatch, node| {
            const STR: ExpectedNode = ExpectedNode::Primitive(PrimitiveKind::String);
            const OPAQUE: ExpectedNode = ExpectedNode::OpaqueUnmodeledPosition;
            assert!(
                check_node(
                    dispatch,
                    node,
                    &ExpectedNode::Object(&[("label", STR), ("made", OPAQUE)])
                )
                .is_empty(),
                "the D01 shape must pin its unmodelled member with the TYPED marker \
                     (measured {})",
                render_node(dispatch, node, 0)
            );
            assert!(
                !check_node(
                    dispatch,
                    node,
                    &ExpectedNode::Object(&[("label", OPAQUE), ("made", OPAQUE)])
                )
                .is_empty(),
                "the marker must REJECT a modelled member — otherwise it matches anything"
            );
            assert!(
                !check_node(
                    dispatch,
                    node,
                    &ExpectedNode::Object(&[("label", STR), ("made", STR)])
                )
                .is_empty(),
                "an opaque member must NOT satisfy a primitive pin"
            );
        });
        // A DIFFERENT opaque error must not satisfy the marker either:
        // the constructor-valued member measures `Opaque` with a
        // NON-UnmodeledPosition query error (the F04 shape), so the pin
        // discriminates the exact typed error, not the opaque class.
        with_flow_node(
            "function base3() { return { label: String } }",
            "base3",
            |dispatch, node| {
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::Object(&[("label", ExpectedNode::OpaqueUnmodeledPosition)])
                    )
                    .is_empty(),
                    "an Opaque node carrying a DIFFERENT query error must NOT satisfy the \
                     UnmodeledPosition marker (measured {})",
                    render_node(dispatch, node, 0)
                );
            },
        );
    }

    /// CONTROL — the `TypeParam` pin at NAME level: the generic return
    /// measures `TypeParam(T)`; the pin accepts `T` and REJECTS a
    /// different name. The trio controls discriminate the VARIANT; this
    /// control discriminates the NAME within the variant.
    #[test]
    fn type_param_expectation_rejects_a_wrong_name() {
        with_flow_node(
            "function makeProps<T>(x: T) { return x }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    check_node(dispatch, node, &ExpectedNode::TypeParam { name: "T" }).is_empty(),
                    "the generic return must measure TypeParam(T) (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !check_node(dispatch, node, &ExpectedNode::TypeParam { name: "U" }).is_empty(),
                    "a TypeParam pin that cannot reject a DIFFERENT name is variant-only"
                );
            },
        );
    }

    /// CONTROL — the `BareRef` pin at NAME level, on a GENUINELY-measured
    /// `BareRef` (an unresolved reference): accepts its own name, rejects
    /// a different name, and stays distinct from `DeclRef`.
    #[test]
    fn bare_ref_expectation_rejects_a_wrong_name() {
        with_flow_node(
            "function makeProps(x: NotDeclaredAnywhere) { return x }",
            "makeProps",
            |dispatch, node| {
                assert!(
                    check_node(
                        dispatch,
                        node,
                        &ExpectedNode::BareRef {
                            name: "NotDeclaredAnywhere"
                        }
                    )
                    .is_empty(),
                    "the unresolved reference must measure BareRef(NotDeclaredAnywhere) \
                     (measured {})",
                    render_node(dispatch, node, 0)
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::BareRef {
                            name: "SomethingElse"
                        }
                    )
                    .is_empty(),
                    "a BareRef pin that cannot reject a DIFFERENT name is variant-only"
                );
                assert!(
                    !check_node(
                        dispatch,
                        node,
                        &ExpectedNode::DeclRef {
                            name: "NotDeclaredAnywhere"
                        }
                    )
                    .is_empty(),
                    "an unresolved reference must NOT satisfy a DeclRef pin — the identities \
                     stay distinct"
                );
            },
        );
    }

    /// CONTROL — the two FIRST-CALL-COLD clauses of `check_boundary`.
    /// `check_boundary` is pure over the measurement, so this control
    /// feeds it a WARM-FIRST trace built exclusively from measured
    /// values: the clean program's own observed warm second call
    /// (`from_cache == true`, zero cold computes — exactly what a third
    /// call reports), re-labelled as call 1. Both cold clauses must fail,
    /// individually identified, and nothing else may fire.
    #[test]
    fn boundary_first_call_cold_clauses_fail_against_a_warm_first_trace() {
        let clean = drive_expect_boundary(
            "",
            "ctl_warm_first",
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            None,
        );
        let m = &clean.boundary;
        assert!(
            m.error.is_none() && m.second_from_cache && m.second_cold_computes == 0,
            "control precondition: the clean program replays warm; measured {m:?}"
        );
        assert_eq!(
            m.json, m.second_json,
            "control precondition: the clean replay projects identically"
        );
        let json = m.json.clone().expect("clean control projects JSON");
        let warm_first = MeasuredBoundary {
            first_from_cache: m.second_from_cache,
            first_cold_computes: m.second_cold_computes,
            degradation: m.degradation,
            json: m.second_json.clone(),
            second_from_cache: m.second_from_cache,
            second_cold_computes: m.second_cold_computes,
            second_degradation: m.second_degradation,
            second_json: m.second_json.clone(),
            error: None,
            error_kind: None,
            second_error: None,
            second_error_kind: None,
        };
        let fails = check_boundary(&json, Degr::None, true, &warm_first);
        assert!(
            fails
                .iter()
                .any(|f| f.contains("call 1 reported from_cache=true")),
            "the FIRST-CALL-COLD clause must fail a warm first call: {fails:?}"
        );
        assert!(
            fails.iter().any(|f| f.contains("cold_computes == 0")),
            "the COLD-COMPUTES clause must fail a zero-compute first call: {fails:?}"
        );
        assert_eq!(
            fails.len(),
            2,
            "only the two first-call-cold clauses may fire against this trace: {fails:?}"
        );
    }

    /// CONTROL — the WARM-PAIR clauses fail INDIVIDUALLY, not only
    /// jointly: `from_cache == false` with zero cold computes trips the
    /// replay-warm clause alone, and `from_cache == true` with nonzero
    /// cold computes trips the zero-cold-computes clause alone. Each
    /// trace is a REAL measurement with the single discriminating field
    /// substituted by another measured value from the same run, stated
    /// inline.
    #[test]
    fn boundary_warm_pair_clauses_fail_individually() {
        // (a) The REAL degraded trace (ReturnOnly ⇒ cold replay), with
        // second_cold_computes set to zero — the only measured field that
        // distinguishes the two warm clauses. Only the replay-warm clause
        // may fire.
        let degraded = drive_expect_boundary(
            "",
            "ctl_warm_pair_degraded",
            DEGRADED_SCRIPT,
            "makeProps",
            None,
        );
        let d = &degraded.boundary;
        assert!(
            d.error.is_none() && !d.second_from_cache && d.second_cold_computes >= 1,
            "control precondition: the degraded program replays COLD; measured {d:?}"
        );
        let degraded_json = d.json.clone().expect("degraded control projects JSON");
        let cold_but_flagless = MeasuredBoundary {
            first_from_cache: d.first_from_cache,
            first_cold_computes: d.first_cold_computes,
            degradation: d.degradation,
            json: d.json.clone(),
            second_from_cache: d.second_from_cache,
            second_cold_computes: 0,
            second_degradation: d.second_degradation,
            second_json: d.second_json.clone(),
            error: None,
            error_kind: None,
            second_error: None,
            second_error_kind: None,
        };
        let fails = check_boundary(
            &degraded_json,
            Degr::UnmodeledPosition,
            true,
            &cold_but_flagless,
        );
        assert!(
            fails.len() == 1 && fails[0].contains("must REPLAY WARM"),
            "the replay-warm clause must fail ALONE when from_cache=false with zero cold \
             computes: {fails:?}"
        );

        // (b) The REAL clean trace, with the second call's compute count
        // set to the SAME trace's measured first-call count (a real
        // nonzero measurement). Only the zero-cold-computes clause may
        // fire.
        let clean = drive_expect_boundary(
            "",
            "ctl_warm_pair_clean",
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            None,
        );
        let c = &clean.boundary;
        assert!(
            c.error.is_none() && c.second_from_cache && c.first_cold_computes >= 1,
            "control precondition: the clean program replays warm after a computing cold call; \
             measured {c:?}"
        );
        let clean_json = c.json.clone().expect("clean control projects JSON");
        let warm_but_computing = MeasuredBoundary {
            first_from_cache: c.first_from_cache,
            first_cold_computes: c.first_cold_computes,
            degradation: c.degradation,
            json: c.json.clone(),
            second_from_cache: c.second_from_cache,
            second_cold_computes: c.first_cold_computes,
            second_degradation: c.second_degradation,
            second_json: c.second_json.clone(),
            error: None,
            error_kind: None,
            second_error: None,
            second_error_kind: None,
        };
        let fails = check_boundary(&clean_json, Degr::None, true, &warm_but_computing);
        assert!(
            fails.len() == 1 && fails[0].contains("cold evaluations"),
            "the zero-cold-computes clause must fail ALONE when a warm replay reports cold \
             work: {fails:?}"
        );
    }

    /// CONTROL — the REPLAY-DRIFT clause and the NO-VALUE clause. The
    /// drifted trace is a real measurement whose second projection is
    /// substituted with a DIFFERENT program's real measured projection;
    /// the no-value trace is a real refusal (the statement-position
    /// IIFE-write program, whose refusal `matrix_cells_hold_their_pins`
    /// pins).
    #[test]
    fn boundary_replay_drift_and_no_value_clauses_fail() {
        let a = drive_expect_boundary(
            "",
            "ctl_drift_a",
            "function makeProps() { return \"a\" as const }",
            "makeProps",
            None,
        );
        let b = drive_expect_boundary(
            "",
            "ctl_drift_b",
            "function makeProps() { return \"zz\" as const }",
            "makeProps",
            None,
        );
        let a_json = a.boundary.json.clone().expect("program A projects JSON");
        let b_json = b.boundary.json.clone().expect("program B projects JSON");
        assert_ne!(
            a_json, b_json,
            "control precondition: the two programs project different JSON"
        );
        let am = &a.boundary;
        let drifted = MeasuredBoundary {
            first_from_cache: am.first_from_cache,
            first_cold_computes: am.first_cold_computes,
            degradation: am.degradation,
            json: am.json.clone(),
            second_from_cache: am.second_from_cache,
            second_cold_computes: am.second_cold_computes,
            second_degradation: am.second_degradation,
            second_json: Some(b_json),
            error: None,
            error_kind: None,
            second_error: None,
            second_error_kind: None,
        };
        let fails = check_boundary(&a_json, Degr::None, true, &drifted);
        assert!(
            fails.len() == 1 && fails[0].contains("replay projection drifted"),
            "the drift clause must fail ALONE when call 2 projects another program's JSON: \
             {fails:?}"
        );

        let refused_script = matrix::iife_position_program(matrix::ExprPosition::Statement);
        let refused = drive_expect_boundary("", "ctl_no_value", &refused_script, "makeProps", None);
        assert!(
            refused.boundary.error.is_some(),
            "control precondition: the statement-position IIFE-write program refuses; measured \
             {:?}",
            refused.boundary
        );
        let fails = check_boundary("{}", Degr::None, true, &refused.boundary);
        assert!(
            fails.len() == 1 && fails[0].contains("returned NO VALUE"),
            "the no-value clause must fail ALONE (and stop) against an Err carrier: {fails:?}"
        );
    }

    /// The stamps are load-bearing: the oracle stamp must name the SAME
    /// pinned checker version the corpus records, and the profile stamp
    /// must name the demand point actually driven.
    #[test]
    fn stamps_match_the_pinned_oracle_and_profile() {
        assert!(
            ORACLE_STAMP.contains(crate::u6_flow_shape_corpus_tests::TSGO_VERSION),
            "ORACLE_STAMP must name the pinned checker version the corpus `checker` columns \
             were measured against"
        );
        assert!(
            PROFILE_STAMP.contains("whole_return"),
            "PROFILE_STAMP must name the demand point every measurement drives"
        );
        assert!(
            ReturnProjectionDemand::whole_return().is_whole_return(),
            "the stamped demand point must BE the whole-return demand"
        );
    }
}
