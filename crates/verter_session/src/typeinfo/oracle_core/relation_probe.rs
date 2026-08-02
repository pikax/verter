//! The v4 `relation_verdict` tuple-wire probe: synthesis, strict wire decode,
//! and operand canonicalization
//! (`docs/arch/ri0-relation-verdict-oracle-addendum.md`).
//!
//! PURE + tsgo-free: builds source text and decodes hover RHS text only. The
//! `oracle-gen` generator drives tsgo over the synthesized probe file; the
//! consumption driver + the offline raw-capture rail re-run the SAME strict
//! decoder without tsgo.
//!
//! The probe shape (FIXED, versioned by the v4 schema):
//!
//! ```ts
//! type __oracle_probe__N = [Source] extends [TargetWithInfer]
//!   ? readonly [true, readonly [readonly [0, "A", A], readonly [1, "B", B]]]
//!   : readonly [false, readonly []];
//! ```
//!
//! The outer tuples prevent union distribution AND `any`'s both-branch
//! behavior, so every capture is the WHOLE-union, single-branch relation
//! judgement. Hover is ONLY the transport: tsgo reduces the conditional and
//! prints the instantiated tuple wire; the STRICT decoder below accepts
//! exactly that tuple grammar (anything else is a loud error, never a
//! guess).

use oxc_allocator::Allocator;
use oxc_ast::ast::{TSType, TSTypeOperatorOperator};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use serde_json::Value;
use verter_type_expr::TypeExpr;

use super::admission;
use super::identity::{
    self, BinderLayoutEntry, FreshnessTag, HostProject, HostSetupKind, InferenceModeTag,
    OracleValueKind, RelationKindTag, RelationPolicyRecord, RelationVerdictIdentity,
    WorkspaceFileRef,
};
use super::normalize::{self, ProjectionModeKind};
use super::probe::probe_name;
use super::query_specs::RelationQuerySpec;

/// The ONE fixed relation-binding projection: every captured `bound` TypeExpr
/// (and both canonical operand ASTs) are lowered + normalized under this mode.
/// A constant, never an identity axis — the v4 family has exactly one
/// projection.
pub(crate) const RELATION_BINDING_PROJECTION: ProjectionModeKind = ProjectionModeKind::Expanded;

/// The reserved binder-ref prefix inside the canonical TARGET operand AST.
/// Each `infer X` position in the target pattern is encoded as a
/// `__oracle_binder__X` type ref (AST-precisely substituted BEFORE lowering) —
/// a closed encoding used ONLY inside the v4 identity's `target_operand` axis.
/// A source operand / wire bound carrying this prefix is rejected.
pub(crate) const BINDER_REF_PREFIX: &str = "__oracle_binder__";

// ---------------------------------------------------------------------------
// The normalized relation-verdict value (the ONE boundary the oracle DTO, the
// wire decoder, and the engine-observation adapter all share)
// ---------------------------------------------------------------------------

/// The relation verdict. Closed set; `Unknown` / `Miss` / `BudgetExceeded` are
/// ENGINE failures, never oracle verdicts, so they have no tag here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelationVerdict {
    Assignable,
    NotAssignable,
}

impl RelationVerdict {
    pub(crate) fn tag(self) -> &'static str {
        match self {
            Self::Assignable => "assignable",
            Self::NotAssignable => "not_assignable",
        }
    }

    /// Inverse of [`tag`] for the strict snapshot decoder. An unknown verdict
    /// string is `None` (closed set).
    #[allow(dead_code)]
    pub(crate) fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "assignable" => Some(Self::Assignable),
            "not_assignable" => Some(Self::NotAssignable),
            _ => None,
        }
    }
}

/// One captured inference binding: the binder's target-pattern preorder
/// ordinal + name, and the bound type as a normalized `TypeExpr` (under
/// [`RELATION_BINDING_PROJECTION`]). NO SemanticNodeId ever appears here.
/// `bound_text` is the wire's original bound SLICE (decode-time capture
/// evidence, used by the generator's constraint check probes; never persisted
/// — the persisted record carries only the normalized `bound`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RelationBinding {
    pub(crate) ordinal: u16,
    pub(crate) name: String,
    pub(crate) bound: TypeExpr,
    pub(crate) bound_text: Option<String>,
}

/// The normalized `ObservedRelationVerdict` boundary matching the oracle DTO:
/// a verdict plus the bindings ordered by target-pattern binder preorder.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RelationVerdictValue {
    pub(crate) verdict: RelationVerdict,
    pub(crate) bindings: Vec<RelationBinding>,
}

// ---------------------------------------------------------------------------
// Probe synthesis (PURE text)
// ---------------------------------------------------------------------------

/// The full probe header line `type __oracle_probe__N = [S] extends [T] ?
/// readonly [true, readonly [<triples>]] : readonly [false, readonly []];`.
/// The binder triples are emitted in the DECLARED binder layout order (the
/// target-pattern binder preorder) — the wire's binding order IS this order.
/// Recorded as `raw_capture.probe_header`; the strict decoder's capture rail
/// re-derives it from the identity as a pure function.
pub(crate) fn relation_probe_header(
    ordinal: u16,
    source_text: &str,
    target_text: &str,
    binder_layout: &[BinderLayoutEntry],
) -> String {
    let triples = binder_layout
        .iter()
        .map(|b| format!("readonly [{}, \"{}\", {}]", b.ordinal, b.name, b.name))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "type {} = [{}] extends [{}] ? readonly [true, readonly [{}]] : readonly [false, readonly []];",
        probe_name(ordinal),
        source_text,
        target_text,
        triples
    )
}

/// The full synthesized probe FILE source for a relation row (one probe per
/// file, so rows never share a workspace-file content hash). Deterministic —
/// a pure function of the spec's operand texts + binder layout.
pub(crate) fn relation_probe_source(
    row_function: &str,
    ordinal: u16,
    source_text: &str,
    target_text: &str,
    binder_layout: &[BinderLayoutEntry],
) -> String {
    format!(
        "// @ai-generated - relation-verdict oracle probe (row {row_function})\n{}\n",
        relation_probe_header(ordinal, source_text, target_text, binder_layout)
    )
}

/// The check-probe alias name for a constrained binder's generation-time
/// bound⊢constraint verification (`__oracle_probe_check__N`). Generator-only
/// (driven by the `oracle-gen` snapshot generator, never the consumption path).
#[cfg(feature = "oracle-gen")]
pub(crate) fn relation_check_probe_name(ordinal: u16) -> String {
    format!("__oracle_probe_check__{ordinal}")
}

/// The check-probe header for one captured constrained binding: re-asks the
/// pinned tsgo whether the captured bound text is assignable to the declared
/// constraint text, through the SAME anti-distribution tuple wire. Driven by
/// the generator ONLY (a SECOND probe file synthesized after the main wire is
/// decoded); a `not_assignable` check verdict is a generation error — a bound
/// violating a present constraint never escapes silently. Generator-only.
#[cfg(feature = "oracle-gen")]
pub(crate) fn relation_check_probe_header(
    ordinal: u16,
    bound_text: &str,
    constraint_text: &str,
) -> String {
    format!(
        "type {} = [{}] extends [{}] ? readonly [true, readonly []] : readonly [false, readonly []];",
        relation_check_probe_name(ordinal),
        bound_text,
        constraint_text
    )
}

/// The full synthesized CHECK-probe file source for a constrained row's
/// bound⊢constraint verification (one check probe per constrained binding, in
/// binder preorder). Deterministic — a pure function of the captured bound
/// texts + the declared constraint texts. Generator-only.
#[cfg(feature = "oracle-gen")]
pub(crate) fn relation_check_probe_source(
    row_function: &str,
    checks: &[(u16, String, String)],
) -> String {
    let mut out = format!(
        "// @ai-generated - relation-verdict oracle constraint-check probes (row {row_function})\n"
    );
    for (ordinal, bound_text, constraint_text) in checks {
        out.push_str(&relation_check_probe_header(
            *ordinal,
            bound_text,
            constraint_text,
        ));
        out.push('\n');
    }
    out
}

// ---------------------------------------------------------------------------
// Binder-ref substitution + operand canonicalization (PURE, tsgo-free)
// ---------------------------------------------------------------------------

/// Why an operand text could not be canonicalized. Every failure is loud —
/// the generator never writes a partial / guessed identity axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OperandCanonError {
    /// The operand text did not parse as a single TS type.
    Parse(String),
    /// The strict OXC lower could not represent the operand losslessly.
    Lower,
    /// The normalizer rejected the lowered operand.
    Normalize(String),
    /// A binder name appeared twice in the target pattern.
    DuplicateBinder(String),
    /// The SOURCE operand carried an `infer` position (sources never bind).
    InferInSource,
    /// An `infer` position carried a DEFAULT (`infer X = …`) — outside the
    /// tuple-wire capture grammar this block seats.
    InferWithDefault(String),
}

/// One binder ref extracted from a target pattern: the declared name plus the
/// `extends <constraint>` constraint TEXT when present (sliced by span BEFORE
/// the whole `TSInferType` node is substituted — the constraint is semantic
/// identity, never erased: an `infer V extends string` and a bare `infer V`
/// must derive DISTINCT canonical identities).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BinderRef {
    pub(crate) name: String,
    pub(crate) constraint: Option<String>,
}

/// AST-precise `infer X [extends C]` → `__oracle_binder__X` substitution over
/// the TARGET pattern text: every `TSInferType` node's span is spliced to the
/// reserved binder ref (a plain type reference, which the shared lowerer
/// accepts) — the binder REF carries no constraint, so the constraint is
/// returned alongside (sliced by span BEFORE splicing) as identity data, never
/// erased. The substituted text parses as an ordinary type, so the canonical
/// operand AST reuses the SAME strict lowerer + normalizer as every other
/// axis. A duplicated binder name is rejected here (target-pattern binder
/// preorder is a SET of names); an `infer X = …` default is outside the
/// capture grammar and rejected.
fn substitute_binder_refs(
    target_text: &str,
    allocator: &Allocator,
) -> Result<(String, Vec<BinderRef>), OperandCanonError> {
    let wrapped = format!("type __oracle_operand__ = {target_text};");
    let ret = Parser::new(allocator, &wrapped, SourceType::ts()).parse();
    if ret.panicked || !ret.errors.is_empty() {
        return Err(OperandCanonError::Parse(target_text.to_string()));
    }
    let Some(alias) = ret.program.body.iter().find_map(|stmt| match stmt {
        oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias)
            if alias.id.name == "__oracle_operand__" =>
        {
            Some(&alias.type_annotation)
        }
        _ => None,
    }) else {
        return Err(OperandCanonError::Parse(target_text.to_string()));
    };

    // Collect every TSInferType span (the extends-clause binder positions),
    // each with its constraint span when present.
    let mut spans: Vec<InferSpanRecord> = Vec::new();
    collect_infer_spans(alias, &mut spans);
    // Splice right-to-left so earlier spans stay valid.
    spans.sort_by_key(|span| std::cmp::Reverse(span.0));
    let mut binders: Vec<BinderRef> = Vec::new();
    let mut out = wrapped.clone();
    for (start, end, name, _, has_default) in &spans {
        if *has_default {
            return Err(OperandCanonError::InferWithDefault(name.clone()));
        }
        out.replace_range(
            *start as usize..*end as usize,
            &format!("{BINDER_REF_PREFIX}{name}"),
        );
    }
    // Binders in FIRST-OCCURRENCE order (the target-pattern binder preorder the
    // declared layout is checked against — name AND constraint at each
    // position).
    for (_, _, name, constraint_span, _) in spans.iter().rev() {
        if binders.iter().any(|b| &b.name == name) {
            return Err(OperandCanonError::DuplicateBinder(name.clone()));
        }
        let constraint =
            constraint_span.map(|(cs, ce)| wrapped[cs as usize..ce as usize].trim().to_string());
        binders.push(BinderRef {
            name: name.clone(),
            constraint,
        });
    }
    // Strip the wrapper back off: the substituted text is the alias RHS.
    let alias_rhs = out
        .strip_prefix("type __oracle_operand__ = ")
        .and_then(|s| s.strip_suffix(';'))
        .ok_or_else(|| OperandCanonError::Parse(target_text.to_string()))?;
    Ok((alias_rhs.to_string(), binders))
}

/// Recursively collect the spans of every `TSInferType` under `ts`. Descends
/// into tuple REST (`...infer R`) and OPTIONAL (`infer H?`) elements (non-plain
/// `TSTupleElement` wrappers `as_ts_type()` skips) and into function REST
/// parameters (`...args: infer A` lives in `FormalParameters.rest`, not
/// `items`) — every `infer` position the corpus's target patterns can carry.
/// One collected `TSInferType` span: (start, end, binder name, constraint
/// span when present, whether a default exists).
type InferSpanRecord = (u32, u32, String, Option<(u32, u32)>, bool);

fn collect_infer_spans(ts: &TSType<'_>, out: &mut Vec<InferSpanRecord>) {
    use oxc_ast::ast::TSType::*;
    match ts {
        TSInferType(infer) => {
            let span = infer.span;
            let constraint = infer
                .type_parameter
                .constraint
                .as_ref()
                .map(|c| (c.span().start, c.span().end));
            out.push((
                span.start,
                span.end,
                infer.type_parameter.name.name.to_string(),
                constraint,
                infer.type_parameter.default.is_some(),
            ));
        }
        TSArrayType(arr) => collect_infer_spans(&arr.element_type, out),
        TSParenthesizedType(p) => collect_infer_spans(&p.type_annotation, out),
        TSTupleType(tuple) => {
            for el in &tuple.element_types {
                match el {
                    oxc_ast::ast::TSTupleElement::TSRestType(rest) => {
                        collect_infer_spans(&rest.type_annotation, out);
                    }
                    oxc_ast::ast::TSTupleElement::TSOptionalType(opt) => {
                        collect_infer_spans(&opt.type_annotation, out);
                    }
                    _ => {
                        if let Some(inner) = el.as_ts_type() {
                            collect_infer_spans(inner, out);
                        }
                    }
                }
            }
        }
        TSUnionType(u) => {
            for arm in &u.types {
                collect_infer_spans(arm, out);
            }
        }
        TSIntersectionType(i) => {
            for arm in &i.types {
                collect_infer_spans(arm, out);
            }
        }
        TSTypeOperatorType(op) => collect_infer_spans(&op.type_annotation, out),
        TSFunctionType(f) => {
            for param in &f.params.items {
                if let Some(ann) = &param.type_annotation {
                    collect_infer_spans(&ann.type_annotation, out);
                }
            }
            if let Some(rest) = &f.params.rest {
                if let Some(ann) = &rest.type_annotation {
                    collect_infer_spans(&ann.type_annotation, out);
                }
            }
            collect_infer_spans(&f.return_type.type_annotation, out);
        }
        TSTypeLiteral(lit) => {
            for member in &lit.members {
                match member {
                    oxc_ast::ast::TSSignature::TSPropertySignature(prop) => {
                        if let Some(ann) = &prop.type_annotation {
                            collect_infer_spans(&ann.type_annotation, out);
                        }
                    }
                    oxc_ast::ast::TSSignature::TSIndexSignature(idx) => {
                        collect_infer_spans(&idx.type_annotation.type_annotation, out);
                    }
                    oxc_ast::ast::TSSignature::TSMethodSignature(m) => {
                        for param in &m.params.items {
                            if let Some(ann) = &param.type_annotation {
                                collect_infer_spans(&ann.type_annotation, out);
                            }
                        }
                        if let Some(rest) = &m.params.rest {
                            if let Some(ann) = &rest.type_annotation {
                                collect_infer_spans(&ann.type_annotation, out);
                            }
                        }
                        if let Some(rt) = &m.return_type {
                            collect_infer_spans(&rt.type_annotation, out);
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// Which operand role is being canonicalized. `Target` runs the binder-ref
/// substitution; `Source` REJECTS any `infer` position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandRole {
    Source,
    Target,
}

/// Canonicalize an operand text to its normalized AST JSON (a v4 identity
/// axis): binder-ref substitution (target only) → strict OXC lower (zero
/// computed-member drops) → normalize under [`RELATION_BINDING_PROJECTION`]
/// → `to_json_value`. PURE + tsgo-free, so the consumption driver's redrive
/// recomputes the same bytes.
pub(crate) fn canonical_operand_ast(
    operand_text: &str,
    role: OperandRole,
) -> Result<Value, OperandCanonError> {
    Ok(canonical_operand_ast_with_binders(operand_text, role)?.0)
}

/// [`canonical_operand_ast`] plus the operand's binder names in FIRST-
/// OCCURRENCE order — the target-pattern binder PREORDER the declared binder
/// layout is checked against (empty for a source operand, which never binds).
pub(crate) fn canonical_operand_ast_with_binders(
    operand_text: &str,
    role: OperandRole,
) -> Result<(Value, Vec<BinderRef>), OperandCanonError> {
    let allocator = Allocator::default();
    let (text, binders) = match role {
        OperandRole::Target => substitute_binder_refs(operand_text, &allocator)?,
        OperandRole::Source => {
            // A source never binds: reject any infer position.
            let (.., binders) = substitute_binder_refs(operand_text, &allocator)?;
            if let Some(first) = binders.first() {
                let _ = first;
                return Err(OperandCanonError::InferInSource);
            }
            (operand_text.to_string(), Vec::new())
        }
    };
    let lowered = admission::lower_hover_rhs(&text).ok_or(OperandCanonError::Lower)?;
    let normalized = normalize::normalize(&lowered, RELATION_BINDING_PROJECTION)
        .map_err(|e| OperandCanonError::Normalize(format!("{e:?}")))?;
    Ok((normalized.to_json_value(), binders))
}

// ---------------------------------------------------------------------------
// The STRICT probe-header inverse (the v4 raw-capture rail's header leg)
// ---------------------------------------------------------------------------

/// Why a recorded `raw_capture.probe_header` was not the versioned tuple-wire
/// synthesis. The synthesis is injective by construction, so the inverse is
/// total over the grammar and rejects everything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeHeaderError {
    /// Not exactly one bare `type <probe_name> = …;` alias (wrong name,
    /// modifiers, type parameters, or extra statements).
    Alias,
    /// The RHS was not `[S] extends [T] ? <true-wire> : <false-wire>` (each of
    /// `S` / `T` a 1-tuple wrapping the operand; wrong shape otherwise).
    ConditionalShape(String),
    /// A true/false branch was not the fixed wire tuple, or a binder triple
    /// was not `readonly [<int>, "<name>", <Name-ref>]` with the type reference
    /// naming exactly the declared binder.
    Wire(String),
    /// The triple ordinals were not exactly `0..n-1` in order.
    OrdinalSequence,
    /// Two triples carried the same binder name.
    DuplicateBinder(String),
}

/// Strictly invert [`relation_probe_header`]: parse the recorded probe header
/// and return `(source_text, target_text, binder_names_in_preorder)`. Every
/// grammar deviation rejects — the consumption rail never guesses which probe
/// produced the capture.
pub(crate) fn parse_probe_header(
    header: &str,
    expected_probe_name: &str,
) -> Result<(String, String, Vec<String>), ProbeHeaderError> {
    let allocator = Allocator::default();
    let trimmed = header.trim();
    let ret = Parser::new(&allocator, trimmed, SourceType::ts()).parse();
    if ret.panicked || !ret.errors.is_empty() {
        return Err(ProbeHeaderError::Alias);
    }
    let mut stmts = ret.program.body.iter();
    let Some(oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias)) = stmts.next() else {
        return Err(ProbeHeaderError::Alias);
    };
    if stmts.next().is_some() || alias.declare || alias.type_parameters.is_some() {
        return Err(ProbeHeaderError::Alias);
    }
    if alias.id.name.as_str() != expected_probe_name {
        return Err(ProbeHeaderError::Alias);
    }

    let TSType::TSConditionalType(cond) = &alias.type_annotation else {
        return Err(ProbeHeaderError::ConditionalShape(
            "RHS is not a conditional type".to_string(),
        ));
    };
    let slice = |ts: &TSType<'_>| -> String {
        let span = ts.span();
        trimmed[span.start as usize..span.end as usize]
            .trim()
            .to_string()
    };
    let source_text = single_element_tuple(&cond.check_type)
        .map(&slice)
        .ok_or_else(|| {
            ProbeHeaderError::ConditionalShape("check side is not a 1-tuple".to_string())
        })?;
    let target_text = single_element_tuple(&cond.extends_type)
        .map(slice)
        .ok_or_else(|| {
            ProbeHeaderError::ConditionalShape("extends side is not a 1-tuple".to_string())
        })?;

    // True branch: `readonly [true, readonly [<triples>]]`.
    let true_tuple = readonly_tuple(&cond.true_type)
        .ok_or_else(|| ProbeHeaderError::Wire("true branch is not a readonly tuple".to_string()))?;
    if true_tuple.element_types.len() != 2 {
        return Err(ProbeHeaderError::Wire(format!(
            "true-branch arity {} != 2",
            true_tuple.element_types.len()
        )));
    }
    let true_lit = true_tuple.element_types[0]
        .as_ts_type()
        .and_then(bool_literal)
        .ok_or_else(|| ProbeHeaderError::Wire("true-branch verdict literal missing".to_string()))?;
    if !true_lit {
        return Err(ProbeHeaderError::Wire(
            "true-branch verdict literal is not `true`".to_string(),
        ));
    }
    let triples_ts = true_tuple.element_types[1].as_ts_type().ok_or_else(|| {
        ProbeHeaderError::Wire("true-branch bindings slot is not a plain type".to_string())
    })?;
    let triples = readonly_tuple(triples_ts).ok_or_else(|| {
        ProbeHeaderError::Wire("true-branch bindings slot is not a readonly tuple".to_string())
    })?;
    let mut names: Vec<String> = Vec::new();
    for (index, el) in triples.element_types.iter().enumerate() {
        let triple_ts = el.as_ts_type().and_then(readonly_tuple).ok_or_else(|| {
            ProbeHeaderError::Wire(format!("triple {index} is not a readonly tuple"))
        })?;
        if triple_ts.element_types.len() != 3 {
            return Err(ProbeHeaderError::Wire(format!(
                "triple {index} arity {} != 3",
                triple_ts.element_types.len()
            )));
        }
        let ordinal = triple_ts.element_types[0]
            .as_ts_type()
            .and_then(uint_literal)
            .ok_or_else(|| {
                ProbeHeaderError::Wire(format!(
                    "triple {index} ordinal is not a non-negative integer"
                ))
            })?;
        if ordinal != index as u64 {
            return Err(ProbeHeaderError::OrdinalSequence);
        }
        let name = triple_ts.element_types[1]
            .as_ts_type()
            .and_then(string_literal)
            .ok_or_else(|| {
                ProbeHeaderError::Wire(format!("triple {index} name is not a string literal"))
            })?;
        // The third element must be a bare type reference naming EXACTLY the
        // declared binder (the synthesis writes `…, "A", A]`).
        let TSType::TSTypeReference(reference) =
            triple_ts.element_types[2].as_ts_type().ok_or_else(|| {
                ProbeHeaderError::Wire(format!("triple {index} binder ref is not a plain type"))
            })?
        else {
            return Err(ProbeHeaderError::Wire(format!(
                "triple {index} third element is not a binder type reference"
            )));
        };
        if !reference.type_name.is_identifier() {
            return Err(ProbeHeaderError::Wire(format!(
                "triple {index} binder ref is a qualified name, not a plain identifier"
            )));
        }
        let Some(ref_ident) = reference.type_name.get_identifier_reference() else {
            return Err(ProbeHeaderError::Wire(format!(
                "triple {index} binder ref is not an identifier"
            )));
        };
        if ref_ident.name.as_str() != name {
            return Err(ProbeHeaderError::Wire(format!(
                "triple {index} binder ref `{}` != declared name `{name}`",
                ref_ident.name
            )));
        }
        if names.contains(&name) {
            return Err(ProbeHeaderError::DuplicateBinder(name));
        }
        names.push(name);
    }

    // False branch: `readonly [false, readonly []]`.
    let false_tuple = readonly_tuple(&cond.false_type).ok_or_else(|| {
        ProbeHeaderError::Wire("false branch is not a readonly tuple".to_string())
    })?;
    if false_tuple.element_types.len() != 2 {
        return Err(ProbeHeaderError::Wire(format!(
            "false-branch arity {} != 2",
            false_tuple.element_types.len()
        )));
    }
    let false_lit = false_tuple.element_types[0]
        .as_ts_type()
        .and_then(bool_literal)
        .ok_or_else(|| {
            ProbeHeaderError::Wire("false-branch verdict literal missing".to_string())
        })?;
    if false_lit {
        return Err(ProbeHeaderError::Wire(
            "false-branch verdict literal is not `false`".to_string(),
        ));
    }
    let empty_ts = false_tuple.element_types[1].as_ts_type().ok_or_else(|| {
        ProbeHeaderError::Wire("false-branch bindings slot is not a plain type".to_string())
    })?;
    let empty = readonly_tuple(empty_ts).ok_or_else(|| {
        ProbeHeaderError::Wire("false-branch bindings slot is not a readonly tuple".to_string())
    })?;
    if !empty.element_types.is_empty() {
        return Err(ProbeHeaderError::Wire(
            "false-branch bindings tuple is not empty".to_string(),
        ));
    }

    Ok((source_text, target_text, names))
}

/// A 1-tuple `[X]`'s inner element (the operand wrappers in the probe header).
/// Anything else (a non-tuple, wrong arity, a non-plain element) is `None`.
fn single_element_tuple<'a>(ts: &'a TSType<'a>) -> Option<&'a TSType<'a>> {
    let TSType::TSTupleType(tuple) = ts else {
        return None;
    };
    let [el] = tuple.element_types.as_slice() else {
        return None;
    };
    el.as_ts_type()
}

/// The canonical comparison form of a decoded relation-verdict value: the
/// canonical JSON of `{ verdict, bindings: [{ ordinal, name, bound }] }` with
/// each bound re-encoded through the real TypeExpr codec. Used by BOTH the
/// raw-capture rail (wire-redecode vs stored value) and the comparison driver.
pub(crate) fn relation_value_canonical_form(value: &RelationVerdictValue) -> String {
    let bindings: Vec<Value> = value
        .bindings
        .iter()
        .map(|b| {
            serde_json::json!({
                "ordinal": b.ordinal,
                "name": b.name,
                "bound": b.bound.to_json_value(),
            })
        })
        .collect();
    super::normalize::canonical_json_string(&serde_json::json!({
        "verdict": value.verdict.tag(),
        "bindings": bindings,
    }))
}

// ---------------------------------------------------------------------------
// The STRICT tuple-wire decoder
// ---------------------------------------------------------------------------

/// Why a hover RHS was not the relation tuple wire. Every deviation from the
/// fixed grammar is a loud error — the decoder never guesses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TupleWireError {
    /// The RHS text did not parse as a single TS type.
    Parse,
    /// The top-level shape was not `readonly [<verdict>, readonly [<triples>]]`
    /// (wrong arity, a missing `readonly`, a labelled/optional/rest scaffold
    /// element, or a non-tuple).
    Shape(String),
    /// Element 0 was not the `true` / `false` boolean literal.
    Verdict,
    /// A binding triple was not `readonly [<int ordinal>, "<name>", <type>]`.
    Triple(String),
    /// The false verdict carried bindings (a false wire's binding tuple must
    /// be empty).
    FalseWithBindings,
    /// Two triples carried the same binder name.
    DuplicateBinder(String),
    /// The ordinal sequence was not exactly `0..n-1` in wire order.
    OrdinalSequence,
    /// A bound type slice failed the strict lower / normalize projection.
    BoundProjection(String),
}

/// Strictly decode the relation tuple wire from a hover RHS text. Accepts
/// EXACTLY:
///
/// ```text
/// readonly [true,  readonly [readonly [0, "A", T0], readonly [1, "B", T1], …]]
/// readonly [false, readonly []]
/// ```
///
/// — outer `readonly` 2-tuple; element 0 the `true`/`false` literal; element 1
/// a `readonly` tuple of triples (empty iff the verdict is false); each triple
/// a `readonly` 3-tuple `[<non-negative int ordinal>, "<name>", <bound type>]`
/// with NO labelled / optional / rest scaffold elements anywhere. Each bound
/// type is sliced by span and projected through the ONE relation-binding
/// projection (strict lower + normalize); a bound that cannot be projected
/// losslessly is a loud error.
pub(crate) fn decode_tuple_wire(rhs: &str) -> Result<RelationVerdictValue, TupleWireError> {
    let allocator = Allocator::default();
    let wrapped = format!("type __oracle_probe__ = {rhs};");
    let ret = Parser::new(&allocator, &wrapped, SourceType::ts()).parse();
    if ret.panicked || !ret.errors.is_empty() {
        return Err(TupleWireError::Parse);
    }
    let Some(alias) = ret.program.body.iter().find_map(|stmt| match stmt {
        oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias)
            if alias.id.name == "__oracle_probe__" =>
        {
            Some(&alias.type_annotation)
        }
        _ => None,
    }) else {
        return Err(TupleWireError::Parse);
    };

    let outer = readonly_tuple(alias)
        .ok_or_else(|| TupleWireError::Shape("top level is not a readonly tuple".to_string()))?;
    if outer.element_types.len() != 2 {
        return Err(TupleWireError::Shape(format!(
            "outer tuple arity {} != 2",
            outer.element_types.len()
        )));
    }
    let verdict_ts = outer.element_types[0]
        .as_ts_type()
        .ok_or_else(|| TupleWireError::Shape("verdict slot is not a plain type".to_string()))?;
    let bindings_ts = outer.element_types[1]
        .as_ts_type()
        .ok_or_else(|| TupleWireError::Shape("bindings slot is not a plain type".to_string()))?;

    let verdict = match bool_literal(verdict_ts) {
        Some(true) => RelationVerdict::Assignable,
        Some(false) => RelationVerdict::NotAssignable,
        None => return Err(TupleWireError::Verdict),
    };

    let binding_tuple = readonly_tuple(bindings_ts).ok_or_else(|| {
        TupleWireError::Shape("bindings slot is not a readonly tuple".to_string())
    })?;
    if verdict == RelationVerdict::NotAssignable && !binding_tuple.element_types.is_empty() {
        return Err(TupleWireError::FalseWithBindings);
    }

    let mut bindings: Vec<RelationBinding> = Vec::with_capacity(binding_tuple.element_types.len());
    let mut names: Vec<String> = Vec::new();
    for (index, el) in binding_tuple.element_types.iter().enumerate() {
        let triple_ts = el
            .as_ts_type()
            .ok_or_else(|| TupleWireError::Triple(format!("triple {index} is not a plain type")))?;
        let triple = readonly_tuple(triple_ts).ok_or_else(|| {
            TupleWireError::Triple(format!("triple {index} is not a readonly tuple"))
        })?;
        if triple.element_types.len() != 3 {
            return Err(TupleWireError::Triple(format!(
                "triple {index} arity {} != 3",
                triple.element_types.len()
            )));
        }
        let ordinal_ts = triple.element_types[0]
            .as_ts_type()
            .ok_or_else(|| TupleWireError::Triple(format!("triple {index} ordinal not a type")))?;
        let name_ts = triple.element_types[1]
            .as_ts_type()
            .ok_or_else(|| TupleWireError::Triple(format!("triple {index} name not a type")))?;
        let bound_ts = triple.element_types[2]
            .as_ts_type()
            .ok_or_else(|| TupleWireError::Triple(format!("triple {index} bound not a type")))?;

        let ordinal = uint_literal(ordinal_ts).ok_or_else(|| {
            TupleWireError::Triple(format!(
                "triple {index} ordinal is not a non-negative integer"
            ))
        })?;
        if ordinal != index as u64 {
            return Err(TupleWireError::OrdinalSequence);
        }
        let name = string_literal(name_ts).ok_or_else(|| {
            TupleWireError::Triple(format!("triple {index} name is not a string literal"))
        })?;
        if names.contains(&name) {
            return Err(TupleWireError::DuplicateBinder(name));
        }
        names.push(name.clone());

        // Slice the ORIGINAL bound bytes by span (spans index into `wrapped`)
        // and project through the ONE relation-binding projection.
        let span = bound_ts.span();
        let bound_text = &wrapped[span.start as usize..span.end as usize];
        if bound_text.contains(BINDER_REF_PREFIX) {
            return Err(TupleWireError::BoundProjection(format!(
                "bound {index} carries the reserved binder-ref prefix"
            )));
        }
        let lowered = admission::lower_hover_rhs(bound_text).ok_or_else(|| {
            TupleWireError::BoundProjection(format!("bound {index} did not lower losslessly"))
        })?;
        let bound = normalize::normalize(&lowered, RELATION_BINDING_PROJECTION).map_err(|e| {
            TupleWireError::BoundProjection(format!("bound {index} failed normalize: {e:?}"))
        })?;
        bindings.push(RelationBinding {
            ordinal: ordinal as u16,
            name,
            bound,
            bound_text: Some(bound_text.to_string()),
        });
    }

    Ok(RelationVerdictValue { verdict, bindings })
}

/// Unwrap one `readonly [<els>]` layer: `TSTypeOperator(Readonly)` over a
/// `TSTupleType`. Anything else (a bare tuple, a different operator, a
/// non-tuple) is `None`.
fn readonly_tuple<'a>(ts: &'a TSType<'a>) -> Option<&'a oxc_ast::ast::TSTupleType<'a>> {
    let TSType::TSTypeOperatorType(op) = ts else {
        return None;
    };
    if op.operator != TSTypeOperatorOperator::Readonly {
        return None;
    }
    let TSType::TSTupleType(tuple) = &op.type_annotation else {
        return None;
    };
    // No labelled / optional / rest scaffold elements: every element must be a
    // plain TSType (the caller inspects each).
    if tuple
        .element_types
        .iter()
        .any(|el| el.as_ts_type().is_none())
    {
        return None;
    }
    Some(tuple)
}

/// The boolean literal of a `true` / `false` type, `None` otherwise.
fn bool_literal(ts: &TSType<'_>) -> Option<bool> {
    let TSType::TSLiteralType(lit) = ts else {
        return None;
    };
    match &lit.literal {
        oxc_ast::ast::TSLiteral::BooleanLiteral(b) => Some(b.value),
        _ => None,
    }
}

/// The non-negative INTEGER value of a numeric literal type, `None` for a
/// non-literal, a negative, or a non-integral literal.
fn uint_literal(ts: &TSType<'_>) -> Option<u64> {
    let TSType::TSLiteralType(lit) = ts else {
        return None;
    };
    match &lit.literal {
        oxc_ast::ast::TSLiteral::NumericLiteral(n) => {
            if n.value >= 0.0 && n.value.fract() == 0.0 && n.value <= u64::MAX as f64 {
                Some(n.value as u64)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// The string value of a string literal type, `None` otherwise.
fn string_literal(ts: &TSType<'_>) -> Option<String> {
    let TSType::TSLiteralType(lit) = ts else {
        return None;
    };
    match &lit.literal {
        oxc_ast::ast::TSLiteral::StringLiteral(s) => Some(s.value.to_string()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The registry-derivable v4 identity (PURE, tsgo-free — shared by the
// `oracle-gen` generator AND the `#[cfg(test)]` consumption driver so both
// derive the SAME bytes from the SAME registry entry)
// ---------------------------------------------------------------------------

/// The canonical workspace path of a relation spec's synthesized probe file
/// (`/fixtures/relation_verdict/<row_function>.ts`). One probe per file, so
/// rows never share a workspace-file content hash. Registry-derivable.
pub(crate) fn relation_probe_canonical_path(row_function: &str) -> String {
    format!("/fixtures/relation_verdict/{row_function}.ts")
}

/// Why a registry relation spec could not derive its v4 identity. Every
/// failure is loud — neither the generator nor the consumption driver ever
/// proceeds on a guessed identity axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RelationSpecError {
    /// An operand text failed canonicalization (parse / lower / normalize /
    /// duplicate binder / `infer` in the source).
    Operand(OperandCanonError),
    /// The declared binder layout's ordinals were not exactly `0..n-1` in
    /// layout (preorder) position.
    BinderOrdinalSequence,
    /// The declared binder layout carried a duplicate name.
    DuplicateBinder(String),
    /// The declared binder layout's names did not match the target pattern's
    /// binder refs in TARGET-PATTERN PREORDER (count or name at a position).
    BinderLayoutMismatch {
        layout: Vec<String>,
        target_refs: Vec<String>,
    },
    /// The declared binder's constraint did not match the target pattern's
    /// `extends <constraint>` at the same position (canonical JSON compared).
    BinderConstraintMismatch {
        position: usize,
        declared: String,
        pattern: String,
    },
}

/// Derive the full v4 [`RelationVerdictIdentity`] from a registry relation
/// spec — PURE + tsgo-free: canonicalize both operand texts (target binder
/// substitution included), validate the declared binder layout against the
/// canonical target's reserved binder refs, synthesize the versioned probe
/// file (its content hash is the single `workspace_files` axis), and pin the
/// only admissible capture axes this block (`assignable` relation, the default
/// policy record, `regular` freshness, `standalone` host).
pub(crate) fn relation_identity_from_spec(
    spec: &RelationQuerySpec,
) -> Result<RelationVerdictIdentity, RelationSpecError> {
    let source_operand = canonical_operand_ast(spec.source_text, OperandRole::Source)
        .map_err(RelationSpecError::Operand)?;
    let (target_operand, target_binders) =
        canonical_operand_ast_with_binders(spec.target_text, OperandRole::Target)
            .map_err(RelationSpecError::Operand)?;

    // The declared binder layout: ordinals exactly 0..n-1 in layout position,
    // names unique, and — compared in TARGET-PATTERN PREORDER — the name AND
    // the constraint at each position must equal the target pattern's binder
    // ref at that position (a sorted set-match would accept a reversed layout
    // and record reversed binder identities/bounds; an erased constraint would
    // alias `infer V extends C` with a bare `infer V`).
    let mut layout: Vec<BinderLayoutEntry> = Vec::with_capacity(spec.binder_layout.len());
    if spec.binder_layout.len() != target_binders.len() {
        return Err(RelationSpecError::BinderLayoutMismatch {
            layout: spec
                .binder_layout
                .iter()
                .map(|b| b.name.to_string())
                .collect(),
            target_refs: target_binders.iter().map(|b| b.name.clone()).collect(),
        });
    }
    for (index, (binder, pattern)) in spec
        .binder_layout
        .iter()
        .zip(target_binders.iter())
        .enumerate()
    {
        if binder.ordinal as usize != index {
            return Err(RelationSpecError::BinderOrdinalSequence);
        }
        if layout
            .iter()
            .any(|b: &BinderLayoutEntry| b.name == binder.name)
        {
            return Err(RelationSpecError::DuplicateBinder(binder.name.to_string()));
        }
        if binder.name != pattern.name {
            return Err(RelationSpecError::BinderLayoutMismatch {
                layout: spec
                    .binder_layout
                    .iter()
                    .map(|b| b.name.to_string())
                    .collect(),
                target_refs: target_binders.iter().map(|b| b.name.clone()).collect(),
            });
        }
        // The constraint is an identity axis: canonicalize BOTH sides (the
        // declared registry text and the target pattern's extracted text)
        // through the SAME operand canonicalization and require equality.
        let canonical_constraint = |text: &str| -> Result<Value, RelationSpecError> {
            canonical_operand_ast(text, OperandRole::Source).map_err(RelationSpecError::Operand)
        };
        let declared = binder.constraint.map(canonical_constraint).transpose()?;
        let pattern_constraint = pattern
            .constraint
            .as_deref()
            .map(canonical_constraint)
            .transpose()?;
        if declared != pattern_constraint {
            return Err(RelationSpecError::BinderConstraintMismatch {
                position: index,
                declared: declared
                    .as_ref()
                    .map(normalize::canonical_json_string)
                    .unwrap_or_else(|| "<none>".to_string()),
                pattern: pattern_constraint
                    .as_ref()
                    .map(normalize::canonical_json_string)
                    .unwrap_or_else(|| "<none>".to_string()),
            });
        }
        layout.push(BinderLayoutEntry {
            ordinal: binder.ordinal,
            name: binder.name.to_string(),
            constraint: pattern_constraint,
        });
    }

    let inference_mode = if layout.is_empty() {
        InferenceModeTag::None
    } else {
        InferenceModeTag::TargetPattern
    };
    let probe_source = relation_probe_source(
        spec.row_function,
        spec.query_ordinal,
        spec.source_text,
        spec.target_text,
        &layout,
    );
    let workspace_files = vec![WorkspaceFileRef {
        path: relation_probe_canonical_path(spec.row_function),
        content_hash: identity::content_hash(&probe_source),
    }];
    let host_setup_kind = match spec.host_project.host_setup_kind {
        super::query_specs::HostSetupKindSpec::Standalone => HostSetupKind::Standalone,
        super::query_specs::HostSetupKindSpec::WorkspaceFootprint => {
            HostSetupKind::WorkspaceFootprint
        }
        super::query_specs::HostSetupKindSpec::PackageBacked => HostSetupKind::PackageBacked,
    };
    Ok(RelationVerdictIdentity {
        row_file: spec.row_file.to_string(),
        row_function: spec.row_function.to_string(),
        query_ordinal: spec.query_ordinal,
        workspace_files,
        source_operand,
        target_operand,
        binder_layout: layout,
        relation: RelationKindTag::Assignable,
        policy: RelationPolicyRecord::default_record(),
        freshness: FreshnessTag::Regular,
        inference_mode,
        host_project: HostProject {
            project_root: spec.host_project.project_root.to_string(),
            workspace_root: spec.host_project.workspace_root.to_string(),
            tsconfig_path: spec.host_project.tsconfig_path.to_string(),
            host_setup_kind,
        },
        oracle_value_kind: OracleValueKind::RelationVerdict,
    })
}

/// Collect the binder names named by reserved `__oracle_binder__X` refs inside
/// a canonical operand AST (deduplicated, ANY order — callers sort).
pub(crate) fn collect_binder_refs(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if map.get("kind").and_then(Value::as_str) == Some("ref") {
                if let Some(name) = map.get("name").and_then(Value::as_str) {
                    if let Some(binder) = name.strip_prefix(BINDER_REF_PREFIX) {
                        let binder = binder.to_string();
                        if !out.contains(&binder) {
                            out.push(binder);
                        }
                    }
                }
            }
            for v in map.values() {
                collect_binder_refs(v, out);
            }
        }
        Value::Array(items) => {
            for v in items {
                collect_binder_refs(v, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
