//! Recursive flow-return expectations, public cold/warm boundary, and
//! the crossed capture-write matrix.
//!
//! Root `NodeShape` cannot distinguish `() => "a"` from `() => "b"`
//! (both `Other`) or `"a" | "b"` from `"a" | undefined` (both `Union`).
//!
//! 1. [`ExpectedNode`] asserts signatures, exact literals, intersections,
//!    and order-insensitive union *sets* to arbitrary depth.
//! 2. Distinct graph variants for `TypeParam` / `DeclRef` / `BareRef`
//!    (raise projects all three to `Ref { name }`).
//! 3. Public boundary via `get_flow_return_type_with_audit`, twice.
//!    First call is cold (`from_cache == false`, `cold_computes >= 1`).
//!    A clean result must replay warm with zero cold computes. A
//!    `ReturnOnly` result must not be admitted warm and must
//!    cold-compute again (`from_cache == false` alone is not replay).
//!    A refusal is never served warm.
//! 4. [`matrix`]: the same capture-write effect regardless of expression
//!    position.
//!
//! Stamped with [`ORACLE_STAMP`] (checker, never `.d.ts`) and
//! [`PROFILE_STAMP`]. A deep measurement that diverges from `checker`
//! is `Verdict::KnownOwed`.
//!
//! Controls in [`expectation_controls`] and [`matrix_outcome_controls`]
//! prove every retained comparison clause can fail. Vocabulary neither
//! a row nor a control exercises is omitted; re-add a form only with a
//! control. `SignatureKind::Call` vs `Construct` is load-bearing: the
//! annotation-typed parameter form (`function makeProps(x: new () => Box)
//! { return x }`) reaches `SignatureKind::Construct`, so a call pin
//! must reject a construct signature. Held by
//! [`expectation_controls::construct_signature_is_distinct_from_call_signature`].
//!
//! `Alias` nodes match nothing (fail-closed). [`Lit`] is not total over
//! [`LiteralValue`]: only `Str`/`Num` are exercised. Boundary-clause
//! controls that need a trace no fresh host can produce build it from
//! measured values — a re-labelled or single-field-substituted real
//! trace, stated inline at each site.

use std::sync::Arc;

use super::{degr_of, upsert, Degr};
use crate::host_flow_return_audit::FlowReturnError;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowGap, FlowReturnFailure, FlowReturnUnsupported, LiteralValue, PrimitiveKind, QueryError,
    ReturnProjectionDemand, SemanticNodeData, SemanticNodeId, SignatureKind,
};
use crate::types::HostConfig;
use crate::{FileLanguage, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

// Oracle / profile stamps

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

// The recursive expectation

/// Exact literal expectation. Not total over [`LiteralValue`]: only
/// variants a corpus row or control exercises. Re-add `Bool` / `BigInt`
/// only together with a control.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Lit {
    Str(&'static str),
    Num(f64),
}

/// Recursive graph-node expectation.
///
/// Matched against [`SemanticNodeData`] at the graph-node level — never
/// the projected `TypeExpr`, which conflates `TypeParam` / `DeclRef` /
/// `BareRef` into `Ref { name }`. `Alias` nodes match nothing
/// (fail-closed); reintroduce transparency only with a control. Every
/// variant is pinned by a corpus row or discriminated by a control.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ExpectedNode {
    /// `SemanticNodeData::Literal` with this exact value.
    Literal(Lit),
    /// `SemanticNodeData::Primitive` with this exact kind.
    Primitive(PrimitiveKind),
    /// `SemanticNodeData::Union` whose constituent set equals this set
    /// (order-insensitive, exact). Duplicate expectations need distinct
    /// constituents.
    Union(&'static [ExpectedNode]),
    /// `SemanticNodeData::Intersection` with these arms, in source order.
    Intersection(&'static [ExpectedNode]),
    /// `SemanticNodeData::Signature` with `SignatureKind::Call`, exact
    /// ordered params, and this return type.
    ///
    /// `kind == Call` is load-bearing: `function makeProps(x: new () => Box)
    /// { return x }` measures `SignatureKind::Construct`, so a call pin
    /// must reject it. Held by
    /// [`expectation_controls::construct_signature_is_distinct_from_call_signature`].
    Signature {
        params: &'static [ExpectedNode],
        ret: &'static ExpectedNode,
    },
    /// `SemanticNodeData::Signature` with `SignatureKind::Construct`
    /// (`new () => T`). Distinct so a construct pin rejects a call
    /// signature.
    ConstructSignature {
        params: &'static [ExpectedNode],
        ret: &'static ExpectedNode,
    },
    /// `SemanticNodeData::Object` whose named member set equals this set;
    /// each value is matched recursively.
    Object(&'static [(&'static str, ExpectedNode)]),
    /// `SemanticNodeData::TypeParam` with this display name — distinct
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

/// Order-insensitive exact set equality: every expected node claims a
/// distinct measured constituent and the counts must match.
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

/// Whether `node` matches `expected`, recursively. Silent; [`check_node`]
/// wraps it with a rendered report.
pub(crate) fn node_matches(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    expected: &ExpectedNode,
    depth: usize,
) -> bool {
    if depth > MATCH_DEPTH_LIMIT {
        return false;
    }
    // No alias deref: an `Alias` node matches nothing. Reintroduce
    // transparency only with a discriminating control.
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
            // `kind == Call` is load-bearing: `x: new () => Box` reaches
            // Construct, and a call pin must reject it. Arity is exact;
            // parameter types are ordered.
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
            // Injective: duplicate expected keys cannot both claim one
            // measured member. Surfaces carry unique keys, so greedy
            // claiming is exact.
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

// Row columns: recursive expectation + public-boundary companion

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
    /// `get_flow_return_type_with_audit` twice:
    ///
    /// * call 1 must produce a value, be cold (`from_cache == false`,
    ///   `cold_computes >= 1`), carry exactly `degradation`, and project
    ///   exactly `json`;
    /// * call 2 must keep call 1's result class and degradation, project
    ///   byte-identically, and report the pinned replay: `warm_replay`
    ///   requires `from_cache == true` and zero cold computes; otherwise
    ///   the result must not be admitted warm (`ReturnOnly` no-poison)
    ///   and must cold-compute again (`from_cache == false` with zero
    ///   cold work is not a cold replay).
    Audit {
        json: &'static str,
        degradation: Degr,
        warm_replay: bool,
    },
    /// `get_flow_return_type_with_audit` twice; both calls refuse. Call 1
    /// refuses cold with exactly `error` (full [`FlowReturnError`]
    /// identity). Call 2 refuses with the same identity, is never served
    /// warm (a cached refusal is the typed-non-admission violation), and
    /// recomputes. Checked by [`check_boundary_refusal`].
    AuditRefusal { error: FlowReturnError },
}

/// What the public boundary actually did across the two calls.
#[derive(Debug)]
pub(crate) struct MeasuredBoundary {
    pub first_from_cache: bool,
    pub first_cold_computes: u32,
    /// First-call typed degradation. `Err` leaves this unset and records `error`.
    pub degradation: Option<Degr>,
    /// Exact projected JSON of the first call's result node.
    pub json: Option<String>,
    pub second_from_cache: bool,
    pub second_cold_computes: u32,
    /// Second-call typed degradation. Drift versus the first call is a
    /// comparison failure.
    pub second_degradation: Option<Degr>,
    /// Exact projected JSON of the second call's result node.
    pub second_json: Option<String>,
    /// Debug of the `Err` arm when the FIRST call produced no value.
    pub error: Option<String>,
    /// First-call typed refusal identity. `error` is presentation.
    pub error_kind: Option<FlowReturnError>,
    /// Second-call `Err` debug, so value ⇄ refusal drift is comparable.
    pub second_error: Option<String>,
    /// Second-call typed refusal identity. Kind drift versus `error_kind`
    /// is a comparison failure.
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

/// Drive one program through the public audited flow-return boundary
/// twice; optionally match the result node while the graph is live.
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
    upsert(
        &host,
        &canonical,
        &crate::u6_flow_shape_corpus_tests::module_script(script),
        FileLanguage::script_ts(),
    );
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

    // Match + render against the live graph before the host drops.
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

/// Second-call replay clauses, shared by [`check_boundary`] and
/// [`matrix::check_cell_outcome`] so the comparators cannot drift:
/// class, degradation, warm pair, cold pair, projection.
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

/// Drive one program through the public audited boundary once; return
/// the live dispatch and result node (`None` on refusal).
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
    upsert(
        &host,
        &canonical,
        &crate::u6_flow_shape_corpus_tests::module_script(script),
        FileLanguage::script_ts(),
    );
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

/// First-call-cold clauses, shared by [`check_boundary`] and
/// [`matrix::check_cell_outcome_measured`] so matrix cells pin coldness
/// at corpus parity.
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

/// Check a measured trace against a [`Boundary::Audit`] pin. Pure over
/// the measurement so controls can prove every clause fails.
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

/// Check a measured trace against [`Boundary::AuditRefusal`]: both calls
/// refuse with the pinned identity, cold each time, never warm-admitted.
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

// Checker-syntax semantic projection

/// Test-only projection of the pinned checker's print syntax, with one
/// comparison rule against the live graph.
///
/// Makes a deep-pinned `checker` column load-bearing:
/// `deep_pinned_rows_semantic_equality_follows_their_verdict` parses it
/// and compares every deep-pinned row against the live graph.
/// `RENDER_INCOMPARABLE` exempts presentation bytes only, never
/// semantic equality.
///
/// Grammar: string/number literals, primitives, bare names, `A | B`,
/// `A & B`, `{ name: T; }`, `(p: T, …) => T`. Unsupported text is a
/// loud parse error — never a silent exemption.
///
/// Unions are order-insensitive exact sets; intersections are source-
/// ordered; objects are exact member sets; a function print is a Call
/// signature only (`new (…) => T` never satisfies it). Parameter names
/// are ignored; types and arity are exact. A reference name matches a
/// resolved `DeclRef` only — `BareRef` / `TypeParam` reach `_ => false`
/// (fail-closed). Re-add those arms only with a control.
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
            // A reference name matches a resolved `DeclRef` only.
            // `BareRef` / `TypeParam` reach `_ => false` (fail-closed).
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
                // A function print is a Call signature. Construct
                // (`new (…) => T`) must never satisfy it.
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

// The crossed capture-write matrix

/// Crossed capture-write / effect / completion position matrix.
///
/// Axes (the flow-analysis contract): binding kind ×
/// write timing × closure depth × expression position × guard kind ×
/// completion container. One shared program generator; each cell pins
/// the checker answer ([`ORACLE_STAMP`]) and the current substrate
/// outcome.
///
/// [`matrix_suite::same_capture_write_cell_is_position_independent`]:
/// the invoked-IIFE capture-write cell must measure identically across
/// every covered expression position.
pub(crate) mod matrix {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum BindingKind {
        Let,
        Var,
        Const,
        Param,
    }

    /// Total: uncovered axis variants are named gaps, not errors.
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

    /// Typed refusal every refusing invoked-IIFE capture-write cell
    /// measures: the sequential evaluator does not represent a
    /// directly-invoked closure statement's captured flow effect. Any
    /// other refusal kind fails the cell pin.
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
    /// Total — see [`WriteTiming`].
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
    /// Total — see [`WriteTiming`].
    #[allow(dead_code)]
    #[derive(Clone, Copy, Debug)]
    pub(crate) enum CellExpectation {
        /// Every covered position must measure EXACTLY this outcome.
        Uniform(CellOutcome),
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
        /// `""` when the pin agrees with the checker; otherwise the
        /// recorded expected-versus-actual gap.
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
    /// A `Value` pin models both calls (class, rendering, degradation,
    /// first-call-cold, replay) via the same clauses as `check_boundary`.
    /// A `NoValue` pin is [`check_boundary_refusal`]: both calls refuse
    /// with the pinned identity, cold, never warm-admitted.
    ///
    /// Cells pin the recursive rendering, not a projected-JSON constant.
    /// Replay still asserts call-2-versus-call-1 projection drift.
    /// Encoding stability is the corpus rows' concern.
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

    /// Comparator half of [`check_cell_outcome`]: takes a measurement so
    /// controls can feed single-field-substituted real traces.
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

// The matrix cells — pinned measurements

// Cells are pinned in `matrix_cells.rs`-style inline consts below the
// suite; see `matrix_suite` for the drivers.

#[cfg(test)]
mod matrix_suite {
    use super::matrix::*;
    use super::*;

    fn dump_mode() -> bool {
        std::env::var("U6_CORPUS_DUMP").is_ok_and(|v| v == "1")
    }

    /// Position cell: let × inside-invoked-IIFE × depth 1. Checker:
    /// every position types `"b"`. Pin is Uniform refusal
    /// (`IIFE_EFFECT_REFUSAL`) across covered positions.
    const IIFE_POSITION_CELL: PositionCell = PositionCell {
        id: "let_iife_write_positions",
        binding: BindingKind::Let,
        timing: WriteTiming::InsideInvokedIife,
        depth: 1,
        guard: GuardKind::None,
        container: Container::None,
        positions: COVERED_POSITIONS,
        checker: "\"b\"",
        expectation: CellExpectation::Uniform(CellOutcome::NoValue {
            error: IIFE_EFFECT_REFUSAL,
        }),
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
                degradation: Degr::FlowGap(FlowGap::ClosureCapture),
                warm_replay: false,
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
                degradation: Degr::FlowGap(FlowGap::ClosureCapture),
                warm_replay: false,
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
                degradation: Degr::FlowGap(FlowGap::ClosureCapture),
                warm_replay: false,
            },
            gap: "the DEEPER-closure (depth 2) write never invalidates the captured read — \
                  the G7 class, wrong-and-warm; owner U6.LOOP_CLOSURE",
        },
        FixedCell {
            id: "let_same_closure_unannotated_write",
            binding: BindingKind::Let,
            timing: WriteTiming::InsideReturnedClosure,
            depth: 1,
            guard: GuardKind::None,
            container: Container::None,
            script: "function makeProps() { let x = \"a\"; return () => { x = \"b\"; return x } }",
            checker: "() => string",
            outcome: CellOutcome::Value {
                rendered: "() => string",
                degradation: Degr::None,
                warm_replay: true,
            },
            gap: "",
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
                degradation: Degr::FlowGap(FlowGap::ClosureCapture),
                warm_replay: false,
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
    /// The uniform pin asserts every position's live outcome is pairwise
    /// identical, so a position-specific effect hook cannot be introduced
    /// without failing here.
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

    #[test]
    fn uniform_iife_effect_refusal_covers_every_position() {
        assert_eq!(IIFE_POSITION_CELL.positions, COVERED_POSITIONS);
        let CellExpectation::Uniform(CellOutcome::NoValue { error }) =
            IIFE_POSITION_CELL.expectation
        else {
            panic!("the invoked-closure position cell must be uniformly refused");
        };
        assert_eq!(error, IIFE_EFFECT_REFUSAL);
        for position in IIFE_POSITION_CELL.positions {
            let measured = measure_cell(
                &format!("uniform_iife__{}", position.id()),
                &iife_position_program(*position),
            );
            assert_eq!(measured.boundary.error_kind, Some(IIFE_EFFECT_REFUSAL));
            assert_eq!(
                measured.boundary.second_error_kind,
                Some(IIFE_EFFECT_REFUSAL)
            );
            assert!(!measured.boundary.first_from_cache);
            assert!(!measured.boundary.second_from_cache);
            assert!(measured.boundary.first_cold_computes >= 1);
            assert!(measured.boundary.second_cold_computes >= 1);
        }
    }
}

// Negative controls — the MATRIX COMPARATOR must be able to fail

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

    /// Outcome-class clauses, both directions.
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

    /// Cached refusal must fail non-admission through the cell
    /// comparator's refusal delegation, not only the underlying clauses.
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

    /// A substituted warm first call must fail first-call-cold through
    /// the cell comparator, at corpus parity.
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

// Negative controls — every expectation form must be able to fail

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

/// A control test written INSIDE a nested function value, over a binding
/// an ENCLOSING frame declares, establishes a narrowing the checker
/// applies to every read that follows it in that nested body. The
/// lowering carries no such fact, so the demanded answer is a SUPERSET —
/// and a superset is only ever admissible behind the typed gap.
///
/// A captured binding is a landing slot like any other here: the
/// evaluator resolves the nested read against the enclosing frame's
/// binding, so classifying it as "no slot to land on" would be a POSITIVE
/// claim of inertness that the checker contradicts.
///
/// The controls are what make this discriminating rather than a blanket
/// refusal of closures: a nested value narrowing its OWN parameter still
/// publishes complete and warm, an outer-frame guard is untouched, and a
/// closure that never mentions the capture keeps its clean result.
#[test]
fn narrowing_over_a_captured_binding_degrades_and_never_warms() {
    let degraded = [
        (
            "a `typeof` guard inside a returned arrow",
            "export {};\nfunction makeProps(x: string | number) { return () => { if (typeof x === \"string\") return x; return 0 } }",
        ),
        (
            "a truthiness guard inside a returned arrow",
            "export {};\nfunction makeProps(x?: string) { return () => { if (x) return x; return 0 } }",
        ),
        (
            "a guard inside an immediately invoked value",
            "export {};\nfunction makeProps(x: string | number) { return (() => { if (typeof x === \"string\") return x; return 0 })() }",
        ),
    ];
    for (case, script) in degraded {
        let measured = drive_expect_boundary("", "cap_deg", script, "makeProps", None);
        assert_eq!(
            measured.boundary.degradation,
            Some(Degr::FlowGap(FlowGap::GuardNarrowing)),
            "{case}: the uncarried narrowing takes the typed gap"
        );
        assert!(
            !measured.boundary.second_from_cache,
            "{case}: a degraded result is never served warm"
        );
    }

    let clean = [
        (
            "a nested value narrowing its OWN parameter",
            "export {};\nfunction makeProps() { return (y: string | number) => { if (typeof y === \"string\") return y; return 0 } }",
        ),
        (
            "the same guard one frame out",
            "export {};\nfunction makeProps(x: string | number) { if (typeof x === \"string\") return x; return 0 }",
        ),
        (
            "a closure that never mentions the capture",
            "export {};\nfunction makeProps(x: string | number) { return () => 1 }",
        ),
    ];
    for (case, script) in clean {
        let measured = drive_expect_boundary("", "cap_ok", script, "makeProps", None);
        assert_eq!(
            measured.boundary.degradation,
            Some(Degr::None),
            "{case}: a carried or absent fact stays complete"
        );
        assert!(
            measured.boundary.second_from_cache,
            "{case}: and replays warm"
        );
    }
}

/// A union arm the graph cannot classify against a runtime guard test
/// (`any`, `unknown`) stays possible on BOTH edges of the test: the
/// checker narrows such an arm, so dropping it fabricates a dead branch
/// and loses that branch's return contributor from a result then
/// certified complete and warm. The sound public outcome is the
/// retained superset carrying the typed `FlowGap::GuardNarrowing`
/// degradation — `ReturnOnly`, two cold computes, zero warm candidates —
/// while a fully classified union keeps its exact, warm, gap-free
/// narrow. Covers the positive and negated `typeof` spellings and the
/// `instanceof` spelling.
#[test]
fn unclassifiable_guard_arms_remain_possible_degrade_and_never_warm() {
    struct Case {
        id: &'static str,
        script: &'static str,
        /// tsc `--strict` checker verdict for the row: the lower bound
        /// the retained superset must cover.
        checker: &'static str,
        rendered: &'static str,
        degradation: Degr,
        /// `true`: clean row — exact value, warm replay, stored
        /// candidate. `false`: degraded row — ReturnOnly, second call
        /// cold again, zero stored candidates.
        warm: bool,
    }
    let cases = [
        Case {
            id: "guard_unclassified_typeof_unknown_positive",
            script: "export function f(x: unknown) { if (typeof x === \"string\") return x; return 0; }",
            checker: "string | 0",
            rendered: "unknown",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "guard_unclassified_typeof_any_positive",
            script: "export function f(x: any) { if (typeof x === \"string\") return x; return 0; }",
            checker: "string | 0",
            rendered: "any",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "guard_unclassified_typeof_unknown_negated",
            script: "export function f(x: unknown) { if (typeof x !== \"string\") return 0; return x; }",
            checker: "0 | string",
            rendered: "unknown",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "guard_unclassified_instanceof_unknown",
            script: "class C { m(): number { return 1; } }\nexport function f(x: unknown) { if (x instanceof C) return x; return 0; }",
            checker: "C | 0",
            rendered: "unknown",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "guard_unclassified_instanceof_any",
            script: "class C { m(): number { return 1; } }\nexport function f(x: any) { if (x instanceof C) return x; return 0; }",
            checker: "C | 0",
            rendered: "any",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "guard_classified_union_control",
            script: "export function f(x: string | number) { if (typeof x === \"string\") return x; return 0; }",
            checker: "string | 0",
            rendered: "Union(string | 0)",
            degradation: Degr::None,
            warm: true,
        },
    ];
    for case in &cases {
        let measured = drive_expect_boundary("", case.id, case.script, "f", None);
        let mut failures = Vec::new();
        if let Some(rendered) = measured.rendered.as_deref() {
            if rendered != case.rendered {
                failures.push(format!(
                    "value drifted: expected {} (a sound cover of checker `{}`), measured {}",
                    case.rendered, case.checker, rendered
                ));
            }
        } else {
            failures.push(format!(
                "the public boundary returned no value: {}",
                measured.boundary.error.as_deref().unwrap_or("<unset>")
            ));
        }
        if measured.boundary.degradation != Some(case.degradation) {
            failures.push(format!(
                "typed degradation drifted: expected {:?}, measured {:?}",
                case.degradation, measured.boundary.degradation
            ));
        }
        failures.extend(first_call_cold_clauses(&measured.boundary));
        failures.extend(replay_clauses(case.warm, &measured.boundary));
        let candidates = flow_return_candidate_count(case.id, case.script);
        if case.warm {
            if candidates == 0 {
                failures.push(
                    "clean row stored ZERO warm candidates — a complete result must warm"
                        .to_owned(),
                );
            }
        } else if candidates != 0 {
            failures.push(format!(
                "degraded row stored {candidates} warm candidate(s) — a guard-gapped result \
                 is ReturnOnly and must store NONE"
            ));
        }
        assert!(failures.is_empty(), "{}:\n{}", case.id, failures.join("\n"));
    }
}

/// Warm-candidate count for one program's `FlowReturn` slot after two
/// public boundary calls on a fresh host.
fn flow_return_candidate_count(id: &str, script: &str) -> usize {
    let host = make_audit_host();
    let canonical = format!("/wb/{id}__slots.ts");
    upsert(
        &host,
        &canonical,
        &crate::u6_flow_shape_corpus_tests::module_script(script),
        FileLanguage::script_ts(),
    );
    let ident = identity(&canonical, "f");
    for _ in 0..2 {
        let _ =
            host.get_flow_return_type_with_audit(&ident, ReturnProjectionDemand::whole_return());
    }
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = crate::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical.as_str()),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("f"),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(&canonical),
        demand: ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract:
            crate::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
    };
    dispatch.graph().slot_candidate_count_for_tests(
        &crate::semantic_query::SemanticQueryKey::FlowReturn(Box::new(key)),
    )
}

/// `"key" in x` over a union arm whose key set the graph cannot decide
/// (a type parameter, an index-signature surface, an unresolvable
/// carrier) must keep that arm possible on BOTH edges. Retention is
/// either EXACT or a typed-gap SUPERSET, and which one the checker
/// gives depends on the edge and on whether a SIBLING arm settles the
/// key:
///
/// - Where the checker also keeps the arm (the index-signature negated
///   edge), reading "cannot decide" as "does not carry the key" would
///   fabricate a dead edge and LOSE a return contributor from a result
///   then certified complete and warm — the subset direction, the one
///   that is never acceptable.
/// - Where the checker resolves the union by a sibling arm that DOES
///   declare the key (`T | { k: number }` on the positive edge, whose
///   measured tsc 7.0.2 verdict is `0 | { k: number; }` — the `T` arm
///   is dropped, NOT intersected with `Record<"k", unknown>`; that
///   intersection is what a NON-union `in` subject gets), retaining the
///   undecidable arm is a deliberate SUPERSET. It is admissible only
///   behind the typed `FlowGap::GuardNarrowing`, which forces
///   `ReturnOnly` and never warms — so a later exact decision cannot be
///   shadowed by a cached wide answer.
///
/// An OPTIONAL member is decided per edge: its arm is retained EXACTLY
/// on the negated edge (a value may lack the key) and retained as a
/// degraded superset on the positive edge (the checker refines the key
/// present). `Impossible`
/// needs positive proof: only a closed surface's required member drops
/// an arm on the negated edge, only a closed key-absent surface on the
/// positive edge — those controls stay exact, gap-free, and warm.
#[test]
fn unclassifiable_in_guard_arms_remain_possible_degrade_and_never_warm() {
    struct Case {
        id: &'static str,
        script: &'static str,
        /// tsc `--strict` checker verdict: the lower bound the result
        /// must cover.
        checker: &'static str,
        rendered: &'static str,
        degradation: Degr,
        warm: bool,
    }
    let cases = [
        // The type parameter is bounded by `object` so the fixture is a
        // program the checker ACCEPTS: `"k" in x` requires an `object`
        // right-hand side, and an unconstrained `T` makes the whole row a
        // rejected program (`TS2322: Type 'T | { k: number; }' is not
        // assignable to type 'object'`) whose "checker verdict" would be a
        // reading off an errored compile. The bound does not change the
        // verdict — both spellings measure `0 | { k: number; }` — it only
        // makes the measurement legitimate.
        //
        // SUPERSET row: the checker drops the `T` arm here (the sibling
        // `{ k: number }` settles the key for the union), while the graph
        // cannot decide whether `T` carries `"k"` and keeps it. Admissible
        // only behind the typed guard gap, ReturnOnly — the checker-exact
        // rows below it carry `Degr::None` and warm.
        Case {
            id: "in_unclassified_type_param_positive",
            script: "export function f<T extends object>(x: T | { k: number }) { if (\"k\" in x) return x; return 0; }",
            checker: "0 | { k: number; }",
            rendered: "Union(TypeParam(T) | { k: number } | 0)",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        // The OPTIONAL arm is not an unclassifiable arm: retention is
        // EXACT on both edges (the checker keeps an optional arm
        // unchanged — measured: the negated edge keeps `A | B`, and the
        // positive fall-through edge keeps `A` with no value refinement),
        // so this row is the optional-retention positive control — clean
        // and warm beside its genuinely unclassifiable siblings.
        Case {
            id: "in_optional_member_negated",
            script: "type A = { k?: number }; type B = { m: string };\nexport function f(x: A | B) { if (!(\"k\" in x)) return x; return 0; }",
            checker: "A | B | 0",
            rendered: "Union(DeclRef(A) | DeclRef(B) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "in_index_signature_arm_keeps_contributor",
            script: "export function f(x: { [s: string]: number } | { m: string }) { if (!(\"k\" in x)) return x; return 0; }",
            checker: "{ [s: string]: number } | { m: string } | 0",
            rendered: "Union({  } | { m: string } | 0)",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "in_required_member_positive_control",
            script: "type A = { k: number }; type B = { m: string };\nexport function f(x: A | B) { if (\"k\" in x) return x; return 0; }",
            checker: "A | 0",
            rendered: "Union(DeclRef(A) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "in_required_member_negated_control",
            script: "type A = { k: number }; type B = { m: string };\nexport function f(x: A | B) { if (!(\"k\" in x)) return x; return 0; }",
            checker: "B | 0",
            rendered: "Union(DeclRef(B) | 0)",
            degradation: Degr::None,
            warm: true,
        },
    ];
    for case in &cases {
        let measured = drive_expect_boundary("", case.id, case.script, "f", None);
        let mut failures = Vec::new();
        if let Some(rendered) = measured.rendered.as_deref() {
            if rendered != case.rendered {
                failures.push(format!(
                    "value drifted: expected {} (a sound cover of checker `{}`), measured {}",
                    case.rendered, case.checker, rendered
                ));
            }
        } else {
            failures.push(format!(
                "the public boundary returned no value: {}",
                measured.boundary.error.as_deref().unwrap_or("<unset>")
            ));
        }
        if measured.boundary.degradation != Some(case.degradation) {
            failures.push(format!(
                "typed degradation drifted: expected {:?}, measured {:?}",
                case.degradation, measured.boundary.degradation
            ));
        }
        failures.extend(first_call_cold_clauses(&measured.boundary));
        failures.extend(replay_clauses(case.warm, &measured.boundary));
        let candidates = flow_return_candidate_count(case.id, case.script);
        if case.warm {
            if candidates == 0 {
                failures.push(
                    "clean row stored ZERO warm candidates — a complete result must warm"
                        .to_owned(),
                );
            }
        } else if candidates != 0 {
            failures.push(format!(
                "degraded row stored {candidates} warm candidate(s) — a guard-gapped result \
                 is ReturnOnly and must store NONE"
            ));
        }
        assert!(failures.is_empty(), "{}:\n{}", case.id, failures.join("\n"));
    }
}

/// `instanceof` narrows by the checker's own per-arm rule — the same
/// rule the `x is T` predicate applies — and degrades ONLY the arms it
/// cannot prove. Positive edge: an arm assignable to the instance type
/// survives as itself; an arm the instance type is assignable to
/// narrows TO the instance type (the downcast reading — dropping it
/// instead fabricated a dead branch that published a wrong value warm);
/// an arm related in neither direction keeps the checker's intersection
/// (the checker keeps such a branch ALIVE — measured `0 | ({ name:
/// string } & Unrel)` from the pinned tsc, not a dead branch). Negated
/// edge: only an arm proved to BE the tested class (node identity with
/// the instance type) drops; structural assignability alone cannot
/// prove derivation (the checker KEEPS a same-shape underived arm), so
/// such an arm is retained with the typed guard gap, ReturnOnly. A
/// generic-class arm the relation oracle cannot decide and a
/// construct-signature-typed right-hand side stay retained + gapped —
/// sound supersets, never warm, never a fabricated dead edge.
#[test]
fn instanceof_narrows_by_the_checker_rule_and_gaps_only_unproven_arms() {
    struct Case {
        id: &'static str,
        script: &'static str,
        /// tsc 7.0.2 `--strict --emitDeclarationOnly` verdict.
        checker: &'static str,
        rendered: &'static str,
        degradation: Degr,
        warm: bool,
    }
    let cases = [
        Case {
            id: "instanceof_downcast_positive",
            script: "class Base { name = \"\" }\nclass Sub extends Base { extra = 1 }\nexport function f(x: Base) { if (x instanceof Sub) return x; return 0; }",
            checker: "0 | Sub",
            rendered: "Union(DeclRef(Sub) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "instanceof_downcast_negated",
            script: "class Base { name = \"\" }\nclass Sub extends Base { extra = 1 }\nexport function f(x: Base) { if (!(x instanceof Sub)) return 0; return x; }",
            checker: "0 | Sub",
            rendered: "Union(DeclRef(Sub) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "instanceof_interface_subject_implementing_class",
            script: "interface Animal { name: string }\nclass Dog implements Animal { name: string = \"\"; bark(): void { } }\nexport function f(x: Animal) { if (x instanceof Dog) return x; return 0; }",
            checker: "0 | Dog",
            rendered: "Union(DeclRef(Dog) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "instanceof_unrelated_structural_arm_intersects_alive",
            script: "class Unrel { tag = 1 }\nexport function f(x: { name: string }) { if (x instanceof Unrel) return x; return 0; }",
            checker: "0 | ({ name: string; } & Unrel)",
            rendered: "Union(Intersection({ name: string } & DeclRef(Unrel)) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "instanceof_negated_identity_arm_drops_proved",
            script: "class Base { name = \"\" }\nclass Sub extends Base { extra = 1 }\nexport function f(x: Sub | string) { if (!(x instanceof Sub)) return x; return 1; }",
            checker: "string | 1",
            rendered: "Union(string | 1)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "instanceof_negated_assignable_underived_arm_retained_gapped",
            script: "interface Doglike { name: string; bark(): void }\nclass Dog { name = \"\"; bark(): void { } }\nexport function f(x: Doglike | number) { if (!(x instanceof Dog)) return x; return 1; }",
            checker: "number | Doglike",
            rendered: "Union(DeclRef(Doglike) | number)",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "instanceof_generic_class_arm_retained_gapped",
            script: "class Box<T> { v!: T }\nexport function f(x: Box<number>) { if (x instanceof Box) return x; return 0; }",
            checker: "0 | Box<number>",
            rendered: "Union(InstantiationRef(Box) | 0)",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        Case {
            id: "instanceof_construct_signature_rhs_stays_gapped",
            script: "class Dog { name = \"\"; bark(): void { } }\nconst D: new () => Dog = Dog;\nexport function f(x: { name: string }) { if (x instanceof D) return x; return 0; }",
            checker: "0 | Dog",
            rendered: "Union({ name: string } | 0)",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
    ];
    for case in &cases {
        let measured = drive_expect_boundary("", case.id, case.script, "f", None);
        let mut failures = Vec::new();
        if let Some(rendered) = measured.rendered.as_deref() {
            if rendered != case.rendered {
                failures.push(format!(
                    "value drifted: expected {} (covering checker `{}`), measured {}",
                    case.rendered, case.checker, rendered
                ));
            }
        } else {
            failures.push(format!(
                "the public boundary returned no value: {}",
                measured.boundary.error.as_deref().unwrap_or("<unset>")
            ));
        }
        if measured.boundary.degradation != Some(case.degradation) {
            failures.push(format!(
                "typed degradation drifted: expected {:?}, measured {:?}",
                case.degradation, measured.boundary.degradation
            ));
        }
        failures.extend(first_call_cold_clauses(&measured.boundary));
        failures.extend(replay_clauses(case.warm, &measured.boundary));
        let candidates = flow_return_candidate_count(case.id, case.script);
        if case.warm {
            if candidates == 0 {
                failures.push(
                    "clean row stored ZERO warm candidates — a complete result must warm"
                        .to_owned(),
                );
            }
        } else if candidates != 0 {
            failures.push(format!(
                "degraded row stored {candidates} warm candidate(s) — a guard-gapped result \
                 is ReturnOnly and must store NONE"
            ));
        }
        assert!(failures.is_empty(), "{}:\n{}", case.id, failures.join("\n"));
    }
}

/// A narrow's iteration domain enumerates THROUGH identity carriers: a
/// subject typed by a union type ALIAS (`type Tag = ... | ...`), an
/// alias of an alias, or a generic alias instantiation contributes the
/// aliased union's arms — the checker's own reading — never ONE opaque
/// arm. Treating the carrier as a single arm made every narrow over an
/// alias-typed subject find nothing to filter and publish the WHOLE
/// alias (a superset of the checker's type, e.g. a switch default edge
/// still carrying the matched case's arm) complete and warm. Every
/// guard family iterates the same domain, so the switch dispatch /
/// remainder / exhaustiveness edges, `===`/`!==` literal equality,
/// `typeof`, `instanceof`, `in`, and truthiness all narrow alias-typed
/// subjects exactly as their inline-union spellings do; the inline
/// control pins that the non-carrier domain is unchanged. Alias arms
/// that are THEMSELVES aliases of non-union bodies stay the authored
/// carrier (`DeclRef(A)`), matching the checker's published name. An
/// alias to `boolean` decomposes into its literal arms for switch
/// coverage exactly as the authored primitive does.
#[test]
fn alias_union_subjects_enumerate_and_narrow_like_the_checker() {
    struct Case {
        id: &'static str,
        script: &'static str,
        /// tsc 7.0.2 `--strict --emitDeclarationOnly` verdict.
        checker: &'static str,
        rendered: &'static str,
        degradation: Degr,
        warm: bool,
    }
    let cases = [
        Case {
            id: "alias_switch_template_default",
            script: "type Tag = `item-${string}` | \"none\";\nexport function f(t: Tag) { switch (t) { case \"none\": return { v: 0 }; default: return { v: t } } }",
            checker: "{ v: number; } | { v: `item-${string}`; }",
            rendered: "Union({ v: number } | { v: TemplateLiteral(…) })",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "alias_switch_matched_arm",
            script: "type Tag = \"a\" | \"b\" | \"c\";\nexport function f(t: Tag) { switch (t) { case \"a\": return t; default: return 0 } }",
            checker: "\"a\" | 0",
            rendered: "Union(\"a\" | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "alias_switch_default_remainder",
            script: "type Tag = \"a\" | \"b\" | \"c\";\nexport function f(t: Tag) { switch (t) { case \"a\": return 0; default: return t } }",
            checker: "\"b\" | \"c\" | 0",
            rendered: "Union(\"b\" | \"c\" | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "alias_switch_exhaustive_no_default",
            script: "type Tag = \"a\" | \"b\";\nexport function f(t: Tag) { switch (t) { case \"a\": return 0; case \"b\": return \"s\" } }",
            checker: "\"s\" | 0",
            rendered: "Union(0 | \"s\")",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "boolean_alias_switch_exhaustive",
            script: "type B = boolean;\nexport function f(t: B) { switch (t) { case true: return 1; case false: return \"s\" } }",
            checker: "\"s\" | 1",
            rendered: "Union(1 | \"s\")",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "alias_disc_union_object_arm",
            script: "type U = { kind: \"a\"; v: number } | { kind: \"b\"; w: string };\ntype A = U;\nexport function f(u: A) { if (u.kind === \"a\") return u; return 0 }",
            checker: "0 | { kind: \"a\"; v: number; }",
            rendered: "Union({ kind: \"a\", v: number } | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "alias_arms_are_aliases_disc",
            script: "type A = { kind: \"a\"; v: number };\ntype B = { kind: \"b\"; w: string };\ntype U = A | B;\nexport function f(u: U) { if (u.kind === \"a\") return u; return 0 }",
            checker: "0 | A",
            rendered: "Union(DeclRef(A) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "alias_of_alias_negated_eq",
            script: "type Inner = \"a\" | \"b\";\ntype Outer = Inner | \"c\";\nexport function f(t: Outer) { if (t !== \"c\") return t; return 0 }",
            checker: "0 | Inner",
            rendered: "Union(\"a\" | \"b\" | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "generic_alias_instantiation_eq",
            script: "type Wrap<T> = T | \"none\";\nexport function f(t: Wrap<\"a\">) { if (t !== \"none\") return t; return 0 }",
            checker: "\"a\" | 0",
            rendered: "Union(\"a\" | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "maybe_generic_alias_undefined_negated",
            script: "type Maybe<T> = T | undefined;\nexport function f(t: Maybe<string>) { if (t !== undefined) return t; return 0 }",
            checker: "string | 0",
            rendered: "Union(string | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "typeof_over_alias_subject",
            script: "type SN = string | number;\ntype T = SN;\nexport function f(t: T) { if (typeof t === \"string\") return t; return t }",
            checker: "string | number",
            rendered: "Union(string | number)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "instanceof_alias_negated_identity",
            script: "class Base { name = \"\" }\nclass Sub extends Base { extra = 1 }\ntype U = Sub | string;\nexport function f(x: U) { if (!(x instanceof Sub)) return x; return 1 }",
            checker: "string | 1",
            rendered: "Union(string | 1)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "in_over_alias_subject",
            script: "type U = { a: number } | { b: string };\nexport function f(x: U) { if (\"a\" in x) return x; return 0 }",
            checker: "0 | { a: number; }",
            rendered: "Union({ a: number } | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "truthy_alias_positive_edge",
            script: "type T = 0 | 1;\nexport function f(t: T) { if (t) return t; return 9 }",
            checker: "1 | 9",
            rendered: "Union(1 | 9)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "non_alias_inline_union_control",
            script: "export function f(t: `item-${string}` | \"none\") { switch (t) { case \"none\": return { v: 0 }; default: return { v: t } } }",
            checker: "{ v: number; } | { v: `item-${string}`; }",
            rendered: "Union({ v: number } | { v: TemplateLiteral(…) })",
            degradation: Degr::None,
            warm: true,
        },
    ];
    for case in &cases {
        let measured = drive_expect_boundary("", case.id, case.script, "f", None);
        let mut failures = Vec::new();
        if let Some(rendered) = measured.rendered.as_deref() {
            if rendered != case.rendered {
                failures.push(format!(
                    "value drifted: expected {} (covering checker `{}`), measured {}",
                    case.rendered, case.checker, rendered
                ));
            }
        } else {
            failures.push(format!(
                "the public boundary returned no value: {}",
                measured.boundary.error.as_deref().unwrap_or("<unset>")
            ));
        }
        if measured.boundary.degradation != Some(case.degradation) {
            failures.push(format!(
                "typed degradation drifted: expected {:?}, measured {:?}",
                case.degradation, measured.boundary.degradation
            ));
        }
        failures.extend(first_call_cold_clauses(&measured.boundary));
        failures.extend(replay_clauses(case.warm, &measured.boundary));
        let candidates = flow_return_candidate_count(case.id, case.script);
        if case.warm {
            if candidates == 0 {
                failures.push(
                    "clean row stored ZERO warm candidates — a complete result must warm"
                        .to_owned(),
                );
            }
        } else if candidates != 0 {
            failures.push(format!(
                "degraded row stored {candidates} warm candidate(s) — a guard-gapped result \
                 is ReturnOnly and must store NONE"
            ));
        }
        assert!(failures.is_empty(), "{}:\n{}", case.id, failures.join("\n"));
    }
}

/// A guard over a property PATH lands its fact at the reference the
/// checker narrows — never deeper-rooted. Measured against the pinned
/// checker: `===` / `!==` (and the switch dispatch, remainder, and
/// exhaustiveness edges) narrow the PARENT reference of the tested
/// property — the root only when the path is one segment deep; at depth
/// two the ROOT keeps every constituent, and selecting a root arm there
/// DROPPED a real contributor (a SUBSET published complete and warm —
/// strictly worse than widening). Truthiness narrows BOTH the tested
/// reference and its parent (per-arm member truthiness, both edges,
/// keeping a broad member on the falsy edge). `typeof`, `instanceof`,
/// and `in` narrow ONLY the tested reference: their root stays whole at
/// every depth, pinned by the controls. Switch coverage relates the
/// parent's projected member per LEAF, so a nested boolean or a
/// member-union discriminant proves exhaustiveness exactly as the
/// checker does.
#[test]
fn deep_path_guards_narrow_the_parent_reference_not_the_root() {
    struct Case {
        id: &'static str,
        script: &'static str,
        /// tsc 7.0.2 `--strict --emitDeclarationOnly` verdict.
        checker: &'static str,
        rendered: &'static str,
        degradation: Degr,
        warm: bool,
    }
    let cases = [
        Case {
            id: "eq2_root_keeps_every_constituent",
            script: "type M1 = { meta: { kind: \"one\" }, a: string };\ntype M2 = { meta: { kind: \"two\" }, b: number };\nexport function f(m: M1 | M2) { if (m.meta.kind === \"one\") { return m } return 0 }",
            checker: "0 | M1 | M2",
            rendered: "Union(DeclRef(M1) | DeclRef(M2) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "eq2_parent_reference_narrows",
            script: "type M1 = { meta: { kind: \"one\" }, a: string };\ntype M2 = { meta: { kind: \"two\" }, b: number };\nexport function f(m: M1 | M2) { if (m.meta.kind === \"one\") { return m.meta } return 0 }",
            checker: "0 | { kind: \"one\"; }",
            rendered: "Union({ kind: \"one\" } | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "eq2_negated_parent_reference_narrows",
            script: "type M1 = { meta: { kind: \"one\" }, a: string };\ntype M2 = { meta: { kind: \"two\" }, b: number };\nexport function f(m: M1 | M2) { if (m.meta.kind !== \"one\") { return m.meta } return 0 }",
            checker: "0 | { kind: \"two\"; }",
            rendered: "Union({ kind: \"two\" } | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "eq1_root_discriminant_control",
            script: "type Q1 = { kind: \"one\", a: string };\ntype Q2 = { kind: \"two\", b: number };\nexport function f(m: Q1 | Q2) { if (m.kind === \"one\") { return m } return 0 }",
            checker: "0 | Q1",
            rendered: "Union(DeclRef(Q1) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "switch2_root_keeps_every_constituent",
            script: "type M1 = { meta: { kind: \"one\" }, a: string };\ntype M2 = { meta: { kind: \"two\" }, b: number };\nexport function f(m: M1 | M2) { switch (m.meta.kind) { case \"one\": return m; default: return 0 } }",
            checker: "0 | M1 | M2",
            rendered: "Union(DeclRef(M1) | DeclRef(M2) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "switch2_parent_dispatch_edges",
            script: "type M1 = { meta: { kind: \"one\" }, a: string };\ntype M2 = { meta: { kind: \"two\" }, b: number };\nexport function f(m: M1 | M2) { switch (m.meta.kind) { case \"one\": return m.meta; default: return m.meta } }",
            checker: "{ kind: \"one\"; } | { kind: \"two\"; }",
            rendered: "Union({ kind: \"two\" } | { kind: \"one\" })",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "switch2_exhaustive_no_default",
            script: "type M1 = { meta: { kind: \"one\" }, a: string };\ntype M2 = { meta: { kind: \"two\" }, b: number };\nexport function f(m: M1 | M2) { switch (m.meta.kind) { case \"one\": return 1; case \"two\": return \"s\" } }",
            checker: "\"s\" | 1",
            rendered: "Union(1 | \"s\")",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "switch2_nested_boolean_exhaustive",
            script: "type W1 = { meta: { flag: true } };\ntype W2 = { meta: { flag: false } };\nexport function f(m: W1 | W2) { switch (m.meta.flag) { case true: return 1; case false: return \"s\" } }",
            checker: "\"s\" | 1",
            rendered: "Union(1 | \"s\")",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "switch1_member_union_exhaustive",
            script: "type X = { kind: \"a\" | \"b\" };\nexport function f(u: X) { switch (u.kind) { case \"a\": return 1; case \"b\": return \"s\" } }",
            checker: "\"s\" | 1",
            rendered: "Union(1 | \"s\")",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "truthy1_literal_discriminant_root",
            script: "type V1 = { ok: true, a: 1 };\ntype V2 = { ok: false, b: 2 };\nexport function f(m: V1 | V2) { if (m.ok) { return m } return 0 }",
            checker: "0 | V1",
            rendered: "Union(DeclRef(V1) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "truthy1_literal_discriminant_negated_root",
            script: "type V1 = { ok: true, a: 1 };\ntype V2 = { ok: false, b: 2 };\nexport function f(m: V1 | V2) { if (m.ok) { return 0 } return m }",
            checker: "0 | V2",
            rendered: "Union(DeclRef(V2) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "truthy1_broad_member_root",
            script: "type Y1 = { v: string, a: 1 };\ntype Y2 = { v: undefined, b: 2 };\nexport function f(m: Y1 | Y2) { if (m.v) { return m } return 0 }",
            checker: "0 | Y1",
            rendered: "Union(DeclRef(Y1) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "truthy1_broad_member_negated_keeps_both",
            script: "type Y1 = { v: string, a: 1 };\ntype Y2 = { v: undefined, b: 2 };\nexport function f(m: Y1 | Y2) { if (m.v) { return 0 } return m }",
            checker: "0 | Y1 | Y2",
            rendered: "Union(DeclRef(Y1) | DeclRef(Y2) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "truthy2_parent_reference_narrows",
            script: "type U1 = { meta: { v: string }, a: 1 };\ntype U2 = { meta: { v: undefined }, b: 2 };\nexport function f(m: U1 | U2) { if (m.meta.v) { return m.meta } return 0 }",
            checker: "0 | { v: string; }",
            rendered: "Union({ v: string } | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "typeof2_root_untouched_control",
            script: "type N1 = { meta: { v: string }, a: 1 };\ntype N2 = { meta: { v: number }, b: 2 };\nexport function f(m: N1 | N2) { if (typeof m.meta.v === \"string\") { return m } return 0 }",
            checker: "0 | N1 | N2",
            rendered: "Union(DeclRef(N1) | DeclRef(N2) | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "typeof2_leaf_reference_narrows_control",
            script: "type N1 = { meta: { v: string }, a: 1 };\ntype N2 = { meta: { v: number }, b: 2 };\nexport function f(m: N1 | N2) { if (typeof m.meta.v === \"string\") { return m.meta.v } return 0 }",
            checker: "string | 0",
            rendered: "Union(string | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "in2_parent_reference_narrows_control",
            script: "type R1 = { meta: { x: 1 }, a: 1 };\ntype R2 = { meta: { y: 2 }, b: 2 };\nexport function f(m: R1 | R2) { if (\"x\" in m.meta) { return m.meta } return 0 }",
            checker: "0 | { x: 1; }",
            rendered: "Union({ x: 1 } | 0)",
            degradation: Degr::None,
            warm: true,
        },
        Case {
            id: "inst2_leaf_reference_narrows_control",
            script: "class C { c = 1 }\ntype S1 = { meta: { v: C }, a: 1 };\ntype S2 = { meta: { v: string }, b: 2 };\nexport function f(m: S1 | S2) { if (m.meta.v instanceof C) { return m.meta.v } return 0 }",
            checker: "0 | C",
            rendered: "Union(DeclRef(C) | 0)",
            degradation: Degr::None,
            warm: true,
        },
    ];
    for case in &cases {
        let measured = drive_expect_boundary("", case.id, case.script, "f", None);
        let mut failures = Vec::new();
        if let Some(rendered) = measured.rendered.as_deref() {
            if rendered != case.rendered {
                failures.push(format!(
                    "value drifted: expected {} (covering checker `{}`), measured {}",
                    case.rendered, case.checker, rendered
                ));
            }
        } else {
            failures.push(format!(
                "the public boundary returned no value: {}",
                measured.boundary.error.as_deref().unwrap_or("<unset>")
            ));
        }
        if measured.boundary.degradation != Some(case.degradation) {
            failures.push(format!(
                "typed degradation drifted: expected {:?}, measured {:?}",
                case.degradation, measured.boundary.degradation
            ));
        }
        failures.extend(first_call_cold_clauses(&measured.boundary));
        failures.extend(replay_clauses(case.warm, &measured.boundary));
        let candidates = flow_return_candidate_count(case.id, case.script);
        if case.warm {
            if candidates == 0 {
                failures.push(
                    "clean row stored ZERO warm candidates — a complete result must warm"
                        .to_owned(),
                );
            }
        } else if candidates != 0 {
            failures.push(format!(
                "degraded row stored {candidates} warm candidate(s) — a guard-gapped result \
                 is ReturnOnly and must store NONE"
            ));
        }
        assert!(failures.is_empty(), "{}:\n{}", case.id, failures.join("\n"));
    }
}

/// A bare truthiness guard consumes the shared demand-scoped
/// truthiness-domain fact (`ClassifyTruthinessDomain`) per enumerated
/// arm: an arm with NO inhabitant on the tested edge leaves the edge, an
/// arm with a proven inhabitant stays, and an edge with NO surviving arm
/// narrows the subject to `never` WITHOUT deleting the branch's
/// syntactic return contributor — the checker keeps `return { v }` as
/// `{ v: never }` (measured), so a dead-branch reading that drops the
/// return publishes a subset of the checker's type. An arm whose domain
/// the authority cannot decide (an unresolved operator carrier) stays on
/// both edges, records the typed guard gap, and never warms. Every
/// `checker` column is measured against the pinned tsc 7.0.2
/// (`--strict --declaration --emitDeclarationOnly`).
#[test]
fn truthiness_domain_facts_narrow_like_the_checker() {
    struct Case {
        id: &'static str,
        script: &'static str,
        /// tsc 7.0.2 `--strict --emitDeclarationOnly` verdict.
        checker: &'static str,
        rendered: &'static str,
        degradation: Degr,
        warm: bool,
    }
    let cases = [
        // The certified falsy-edge fact defect: a template-literal type
        // with a non-empty literal prefix has NO falsy inhabitant, so
        // neither `Tag` arm survives the falsy edge — yet the syntactic
        // `return { v }` still contributes, typed `{ v: never }`.
        Case {
            id: "truthy3_template_prefix_falsy_edge_is_never",
            script: "type Tag = `item-${string}` | \"none\";\nexport function f(v: Tag) { if (v) return 1; return { v } }",
            checker: "1 | { v: never; }",
            rendered: "Union(1 | { v: never })",
            degradation: Degr::None,
            warm: true,
        },
        // The negated spelling consumes the same falsy bit on the
        // then-edge — a swapped truthy/falsy mapping fails exactly here.
        Case {
            id: "truthy3_template_prefix_negated_edge_is_never",
            script: "type Tag = `item-${string}` | \"none\";\nexport function f(v: Tag) { if (!v) return { v }; return 1 }",
            checker: "1 | { v: never; }",
            rendered: "Union({ v: never } | 1)",
            degradation: Degr::None,
            warm: true,
        },
        // `${number}` renders only non-empty strings ("0", "NaN", …):
        // no falsy inhabitant even though every quasi is empty.
        Case {
            id: "truthy3_number_template_falsy_edge_is_never",
            script: "export function f(v: `${number}`) { if (v) return 1; return { v } }",
            checker: "1 | { v: never; }",
            rendered: "Union(1 | { v: never })",
            degradation: Degr::None,
            warm: true,
        },
        // `${string}` contains `""`: the falsy edge keeps the arm. This
        // is the overreach control for the non-empty-prefix rule.
        Case {
            id: "truthy3_string_template_keeps_falsy_edge",
            script: "export function f(v: `${string}`) { if (v) return 1; return { v } }",
            checker: "1 | { v: string; }",
            rendered: "Union(1 | { v: TemplateLiteral(…) })",
            degradation: Degr::None,
            warm: true,
        },
        // A bare broad `string` has both a truthy and a falsy inhabitant
        // (`""`): both edges keep it, unrefined, clean and warm — the
        // discriminating pair's second half.
        Case {
            id: "truthy3_bare_string_keeps_both_edges",
            script: "export function f(v: string) { if (v) return 1; return { v } }",
            checker: "1 | { v: string; }",
            rendered: "Union(1 | { v: string })",
            degradation: Degr::None,
            warm: true,
        },
        // An intersection with an object arm has no falsy inhabitant
        // (every inhabitant would inhabit the always-truthy arm), so the
        // falsy edge drops it while `number` stays.
        Case {
            id: "truthy3_object_intersection_falsy_edge_dropped",
            script: "export function f(v: ({ a: 1 } & { b: 2 }) | number) { if (v) return 1; return { v } }",
            checker: "1 | { v: number; }",
            rendered: "Union(1 | { v: number })",
            degradation: Degr::None,
            warm: true,
        },
        // A branded primitive intersection (`string & {x: 1}`) keeps
        // BOTH edges: the branded `""` inhabits the falsy bucket, so the
        // buckets compose by per-bucket OR — the arm survives the truthy
        // edge too (a fold that absorbs into uninhabited drops it there).
        Case {
            id: "truthy3_branded_string_intersection_keeps_both_edges",
            script: "export function f(v: (string & { x: 1 }) | number) { if (v) return { v }; return 1 }",
            checker: "1 | { v: number | (string & { x: 1; }); }",
            rendered: "Union(1 | { v: Union(Intersection(string & { x: 1 }) | number) })",
            degradation: Degr::None,
            warm: true,
        },
        // The MEMBERLESS `{}` admits primitives (`""`, `0`, `false` are
        // all assignable), so BOTH edges keep it — a surface with any
        // member (even optional) stays object-only instead.
        Case {
            id: "truthy3_empty_object_surface_keeps_both_edges",
            script: "export function f(v: {} | 0) { if (v) return 1; return { v } }",
            checker: "1 | { v: 0 | {}; }",
            rendered: "Union(1 | { v: Union({  } | 0) })",
            degradation: Degr::None,
            warm: true,
        },
        // An unresolved operator carrier (`keyof T` over an open `T`)
        // has an UNDECIDED domain: the checker keeps it, the narrow
        // keeps it too, and the undecided fact records the typed guard
        // gap — the result never warms.
        Case {
            id: "truthy3_undecided_carrier_keeps_arm_and_degrades",
            script: "export function f<T>(v: keyof T | 0) { if (v) return 1; return { v } }",
            checker: "1 | { v: 0 | keyof T; }",
            rendered: "Union(1 | { v: Union(KeyOf(TypeParam(T)) | 0) })",
            degradation: Degr::FlowGap(FlowGap::GuardNarrowing),
            warm: false,
        },
        // A type parameter classifies through its CONSTRAINT's domain: a
        // truthy-only constraint (`"a"`) leaves the falsy edge with no
        // surviving arm.
        Case {
            id: "truthy3_constrained_param_falsy_edge_is_never",
            script: "export function f<T extends \"a\">(v: T | \"x\") { if (v) return 1; return { v } }",
            checker: "1 | { v: never; }",
            rendered: "Union(1 | { v: never })",
            degradation: Degr::None,
            warm: true,
        },
        // An UNCONSTRAINED parameter's domain is `unknown`'s: both edges
        // keep it, decided, clean and warm.
        Case {
            id: "truthy3_unconstrained_param_keeps_both_edges",
            script: "export function f<T>(v: T | 0) { if (v) return 1; return { v } }",
            checker: "1 | { v: 0 | T; }",
            rendered: "Union(1 | { v: Union(TypeParam(T) | 0) })",
            degradation: Degr::None,
            warm: true,
        },
    ];
    for case in &cases {
        let measured = drive_expect_boundary("", case.id, case.script, "f", None);
        let mut failures = Vec::new();
        if let Some(rendered) = measured.rendered.as_deref() {
            if rendered != case.rendered {
                failures.push(format!(
                    "value drifted: expected {} (covering checker `{}`), measured {}",
                    case.rendered, case.checker, rendered
                ));
            }
        } else {
            failures.push(format!(
                "the public boundary returned no value: {}",
                measured.boundary.error.as_deref().unwrap_or("<unset>")
            ));
        }
        if measured.boundary.degradation != Some(case.degradation) {
            failures.push(format!(
                "typed degradation drifted: expected {:?}, measured {:?}",
                case.degradation, measured.boundary.degradation
            ));
        }
        failures.extend(first_call_cold_clauses(&measured.boundary));
        failures.extend(replay_clauses(case.warm, &measured.boundary));
        let candidates = flow_return_candidate_count(case.id, case.script);
        if case.warm {
            if candidates == 0 {
                failures.push(
                    "clean row stored ZERO warm candidates — a complete result must warm"
                        .to_owned(),
                );
            }
        } else if candidates != 0 {
            failures.push(format!(
                "degraded row stored {candidates} warm candidate(s) — a guard-gapped result \
                 is ReturnOnly and must store NONE"
            ));
        }
        assert!(failures.is_empty(), "{}:\n{}", case.id, failures.join("\n"));
    }
}

/// One member-object wrapper for the exact boundary JSON pins below:
/// `{ v: <ty> }` exactly as the projector serialises a fresh literal
/// return object with the single member `v`.
#[cfg(test)]
fn single_v_object_json(ty: &str) -> String {
    format!(
        r#"{{"kind":"object","properties":[{{"excessOrigin":"freshOwn","key":{{"kind":"string","value":"v"}},"memberKind":"property","optional":false,"readonly":false,"ty":{ty}}}]}}"#
    )
}

/// `as const` literal identity through EVOLVING-variable assignments and
/// joins (checker-measured, tsgo 7.0.2 `--strict`): an assignment into a
/// binding with no declared authority widens EXACTLY the fresh
/// positions — a bare literal, a fresh ternary arm, a read of a
/// widening-literal `const` — and preserves every pinned one — a
/// const-asserted literal, a pinned-const read, a callee's literal
/// return. `switch`, `if`/`else`, straight-line reassignment and ternary
/// RHS all route through the ONE assignment authority; a fresh and a
/// pinned spelling of the SAME literal collapse to the pinned literal
/// (measured: `c ? 1 : 1 as const` reads `1`, never `number`). The
/// `if`/`else` twin keeps its sound `ConditionalVarDefinition`
/// fail-close (the substrate cannot yet prove the never-assigned path
/// empty), but its VALUE now carries the pinned literals.
#[test]
fn as_const_literal_identity_survives_evolving_assignments_and_joins() {
    let lit_s = |v: &str| format!(r#"{{"kind":"literal","literalKind":"string","value":"{v}"}}"#);
    let lit_n = |v: &str| format!(r#"{{"kind":"literal","literalKind":"number","value":{v}}}"#);
    let prim = |name: &str| format!(r#"{{"kind":"primitive","name":"{name}"}}"#);
    let union2 = |a: &str, b: &str| format!(r#"{{"kind":"union","types":[{a},{b}]}}"#);
    let union3 = |a: &str, b: &str, c: &str| format!(r#"{{"kind":"union","types":[{a},{b},{c}]}}"#);
    let cases: [(&str, &str, String, Degr, bool); 12] = [
        (
            "switch_asconst_join",
            "function f(n: number) { let v; switch (n) { case 1: v = \"r\" as const; break; case 2: v = 2 as const; break; default: v = true as const } return { v } }",
            single_v_object_json(&union3(
                &lit_s("r"),
                &lit_n("2.0"),
                r#"{"kind":"literal","literalKind":"boolean","value":true}"#,
            )),
            Degr::None,
            true,
        ),
        (
            "ifelse_asconst_join_keeps_conditional_fail_close",
            "function f(n: number) { let v; if (n > 0) { v = \"p\" as const } else { v = 1 as const } return { v } }",
            single_v_object_json(&union2(&lit_s("p"), &lit_n("1.0"))),
            Degr::ConditionalVarDefinition,
            false,
        ),
        (
            "straightline_reassign_last_pinned_write_wins",
            "function f() { let v; v = \"r\" as const; v = 2 as const; return { v } }",
            single_v_object_json(&lit_n("2.0")),
            Degr::None,
            true,
        ),
        (
            "pinned_const_read_stays_pinned",
            "function f() { const w = 1 as const; let v; v = w; return { v } }",
            single_v_object_json(&lit_n("1.0")),
            Degr::None,
            true,
        ),
        (
            "widening_const_read_still_widens",
            "function f() { const w = 1; let v; v = w; return { v } }",
            single_v_object_json(&prim("number")),
            Degr::None,
            true,
        ),
        (
            "callee_literal_return_is_not_fresh",
            "function g() { return 1 as const }\nfunction f() { let v; v = g(); return { v } }",
            single_v_object_json(&lit_n("1.0")),
            Degr::None,
            true,
        ),
        (
            "ternary_fresh_arms_widen",
            "function f(n: number) { let v; v = n > 0 ? \"r\" : 2; return { v } }",
            single_v_object_json(&union2(&prim("string"), &prim("number"))),
            Degr::None,
            true,
        ),
        (
            "ternary_pinned_arms_stay",
            "function f(n: number) { let v; v = n > 0 ? \"r\" as const : 2 as const; return { v } }",
            single_v_object_json(&union2(&lit_s("r"), &lit_n("2.0"))),
            Degr::None,
            true,
        ),
        (
            "ternary_mixed_arms_split_per_arm",
            "function f(n: number) { let v; v = n > 0 ? (\"r\" as const) : 2; return { v } }",
            single_v_object_json(&union2(&lit_s("r"), &prim("number"))),
            Degr::None,
            true,
        ),
        (
            "same_literal_fresh_and_pinned_collapse_to_pinned",
            "function f(c: boolean) { let v; v = c ? 1 : 1 as const; return { v } }",
            single_v_object_json(&lit_n("1.0")),
            Degr::None,
            true,
        ),
        (
            "satisfies_preserves_freshness",
            "function f() { let v; v = \"r\" satisfies string; return { v } }",
            single_v_object_json(&prim("string")),
            Degr::None,
            true,
        ),
        (
            "switch_bare_literals_still_widen",
            "function f(n: number) { let v; switch (n) { case 1: v = \"r\"; break; case 2: v = 2; break; default: v = true } return { v } }",
            single_v_object_json(&union3(
                &prim("string"),
                &prim("number"),
                &prim("boolean"),
            )),
            Degr::None,
            true,
        ),
    ];
    let mut report = Vec::new();
    for (case, script, json, degradation, warm) in &cases {
        let measured = drive_expect_boundary("", "evolve_fresh", script, "f", None);
        let failures = check_boundary(json, *degradation, *warm, &measured.boundary);
        if !failures.is_empty() {
            report.push(format!(
                "== {case}:\n{}\nrendered: {}",
                failures.join("\n"),
                measured.rendered.as_deref().unwrap_or("<none>")
            ));
        }
    }
    assert!(report.is_empty(), "\n{}", report.join("\n"));
}

/// `"k" in x` models KEY PRESENCE separately from value
/// non-`undefined`-ness, and a member VALUE READ carries the optional
/// member's own absent-key `undefined` (checker-measured, tsgo 7.0.2
/// `--strict`, `exactOptionalPropertyTypes` NOT in the oracle profile):
/// the positive edge keeps an optional arm UNCHANGED — the checker does
/// not refine the value to non-`undefined` (`if ("k" in x) x.k` reads
/// `string | undefined`, byte-identical to the guard-free read) — so
/// retention is exact and publishes clean and warm; a REQUIRED member
/// gains no fabricated `undefined`; an explicit `| undefined` gains no
/// duplicate; an arm whose key set the graph cannot decide (an
/// index-signature surface) still fails closed with the typed guard gap
/// and never warms.
#[test]
fn in_guard_presence_is_separate_from_value_undefined() {
    let obj_v = |ty: &str| single_v_object_json(ty);
    let str_or_undef = r#"{"kind":"union","types":[{"kind":"primitive","name":"string"},{"kind":"primitive","name":"undefined"}]}"#;
    // `return { v: 0 }`'s member literal widens at the member position
    // (fresh-literal object member) — the checker's own `{ v: number }`.
    let num = r#"{"kind":"primitive","name":"number"}"#;
    let union2 = |a: &str, b: &str| format!(r#"{{"kind":"union","types":[{a},{b}]}}"#);
    let cases: [(&str, &str, String, Degr, bool); 8] = [
        (
            "optional_member_positive_edge_keeps_undefined",
            "type T = { k?: string }\nfunction f(x: T) { if (\"k\" in x) { return { v: x.k } } return { v: 0 } }",
            union2(&obj_v(str_or_undef), &obj_v(num)),
            Degr::None,
            true,
        ),
        (
            "union_optional_arm_selected_keeps_undefined",
            "type A = { k?: string }; type B = { n: number }\nfunction f(x: A | B) { if (\"k\" in x) { return { v: x.k } } return { v: 0 } }",
            union2(&obj_v(str_or_undef), &obj_v(num)),
            Degr::None,
            true,
        ),
        (
            "negated_edge_keeps_optional_arm_and_undefined",
            "type T = { k?: string }\nfunction f(x: T) { if (\"k\" in x) { return { v: 0 } } return { v: x.k } }",
            union2(&obj_v(num), &obj_v(str_or_undef)),
            Degr::None,
            true,
        ),
        (
            "mixed_optional_and_required_arms_union_their_reads",
            "type A = { k?: string }; type B = { k: number }\nfunction f(x: A | B) { if (\"k\" in x) { return { v: x.k } } return { v: 0 } }",
            union2(
                &obj_v(r#"{"kind":"union","types":[{"kind":"primitive","name":"string"},{"kind":"primitive","name":"number"},{"kind":"primitive","name":"undefined"}]}"#),
                &obj_v(num),
            ),
            Degr::None,
            true,
        ),
        (
            "guard_free_optional_read_carries_undefined",
            "type T = { k?: string }\nfunction f(x: T) { return { v: x.k } }",
            obj_v(str_or_undef),
            Degr::None,
            true,
        ),
        (
            "terminal_hop_after_required_hop_carries_undefined",
            "type Inner = { k?: string }; type T = { a: Inner }\nfunction f(x: T) { return { v: x.a.k } }",
            obj_v(str_or_undef),
            Degr::None,
            true,
        ),
        // The two rows below sit on a REQUIRED single-arm subject, whose
        // negated `in` edge no arm survives. That edge stays ALIVE with
        // the subject read as `never`, so the fall-through
        // `return { v: 0 }` — which never reads the subject — keeps its
        // own contribution (measured: `{ v: string; } | { v: number; }`),
        // and the kept arm's value stays EXACT: no fabricated
        // `undefined` on a required member, no duplicate `undefined` on
        // an explicit one.
        (
            "required_member_gains_no_undefined",
            "type T = { k: string }\nfunction f(x: T) { if (\"k\" in x) { return { v: x.k } } return { v: 0 } }",
            union2(
                &obj_v(r#"{"kind":"primitive","name":"string"}"#),
                &obj_v(num),
            ),
            Degr::None,
            true,
        ),
        (
            "explicit_undefined_gains_no_duplicate",
            "type T = { k: string | undefined }\nfunction f(x: T) { if (\"k\" in x) { return { v: x.k } } return { v: 0 } }",
            union2(&obj_v(str_or_undef), &obj_v(num)),
            Degr::None,
            true,
        ),
    ];
    let mut report = Vec::new();
    for (case, script, json, degradation, warm) in &cases {
        let measured = drive_expect_boundary("", "in_presence", script, "f", None);
        let failures = check_boundary(json, *degradation, *warm, &measured.boundary);
        if !failures.is_empty() {
            report.push(format!(
                "== {case}:\n{}\nrendered: {}",
                failures.join("\n"),
                measured.rendered.as_deref().unwrap_or("<none>")
            ));
        }
    }
    assert!(report.is_empty(), "\n{}", report.join("\n"));

    // An arm whose runtime key set the graph cannot decide — an
    // index-signature surface — still fails closed: the typed guard gap
    // rides the result and it NEVER warms (the fold refuses to claim a
    // surface it cannot prove).
    let measured = drive_expect_boundary(
        "",
        "in_presence_unknown",
        "type T = { [key: string]: number }\nfunction f(x: T) { if (\"k\" in x) { return { v: x.k } } return { v: 0 } }",
        "f",
        None,
    );
    assert_eq!(
        measured.boundary.degradation,
        Some(Degr::FlowGap(FlowGap::GuardNarrowing)),
        "an undecidable key set keeps the typed guard gap"
    );
    assert!(
        !measured.boundary.second_from_cache,
        "an undecidable key set is never admitted warm"
    );
}
