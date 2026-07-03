//! Snapshot-backed span recovery: recover the authored spans an IR struct puts
//! in its `Eq`/`Hash` identity, from a retained parse, via a producer-emitted
//! origin locator — BEFORE any identity / interning.
//!
//! Member spans participate in node identity (`ObjectProperty` /
//! `MethodSignature` / `FunctionExpr` / `IndexSignature` derive `Eq`/`Hash` over
//! their span field; `FunctionParam` includes `.span` in its hand-written
//! identity). A closed fact stores no `Span` (the `NoStoredSpan` contract), so a
//! fact that reconstructs such a node carries a [`MemberSpansOrigin`] /
//! [`IndexSignatureSpansOrigin`] / [`FunctionSpansOrigin`] /
//! [`FunctionParamSpanOrigin`] and recovers the exact spans here.
//!
//! Worker-purity split: the PURE core
//! ([`recover_member_spans_from_program`] & siblings) walks a re-borrowed
//! `&Program` sub-position named by the origin path and reads the authored
//! spans — no host / dispatch / service re-entry. The outer helpers compose that
//! pure core with the lease-pinned `DeclLoweringService::run` path.
//!
//! HARD BOUNDARY: these helpers recover ONLY spans. They never lower to a
//! `SemanticNodeId`, touch the `SemanticGraphStore`, memoize under
//! `LocatorLoweringKey`, build a locator lowerer, or reroute
//! `lower_decl_body_with_provenance`.

#![allow(dead_code)]

use std::sync::Arc;

use oxc_ast::ast::{Program, Statement, TSSignature, TSType};
use oxc_span::GetSpan;
use verter_span::Span;
use verter_type_expr::span_origins::{
    FunctionParamSelector, FunctionParamSpanOrigin, FunctionSpansOrigin, IndexSignatureSpansOrigin,
    MemberSpansOrigin,
};
use verter_type_expr::{FunctionSpans, IndexSignatureSpans, MemberSpans};

use crate::decl_lowering::{DeclLoweringService, SnapshotKey};

// ---------------------------------------------------------------------------
// Pure cores — `&Program` + origin → spans. Independently callable/testable.
// ---------------------------------------------------------------------------

/// Recover the `MemberSpans` (declaration / name / type-annotation spans) of the
/// authored member named by `origin`. A synthetic origin recovers the default
/// (all-absent) spans; an authored origin whose path does not resolve (stale /
/// out-of-range) also falls back to the default rather than fabricating a span.
#[must_use]
pub(crate) fn recover_member_spans_from_program(
    program: &Program<'_>,
    origin: &MemberSpansOrigin,
) -> MemberSpans {
    let (anchor, member_path) = match origin {
        MemberSpansOrigin::Authored {
            anchor,
            member_path,
        } => (anchor, member_path),
        MemberSpansOrigin::Synthetic(_) => return MemberSpans::default(),
    };
    let Some(sig) = resolve_member(program, anchor.contributor_index, member_path) else {
        return MemberSpans::default();
    };
    match sig {
        TSSignature::TSPropertySignature(prop) => MemberSpans {
            declaration: Some(prop.span.into()),
            name: Some(prop.key.span().into()),
            type_annotation: prop
                .type_annotation
                .as_ref()
                .map(|ta| ta.type_annotation.span().into()),
        },
        TSSignature::TSMethodSignature(method) => MemberSpans {
            declaration: Some(method.span.into()),
            name: Some(method.key.span().into()),
            type_annotation: None,
        },
        _ => MemberSpans::default(),
    }
}

/// Recover the `IndexSignatureSpans` of the authored index-signature member named
/// by `origin`.
#[must_use]
pub(crate) fn recover_index_signature_spans_from_program(
    program: &Program<'_>,
    origin: &IndexSignatureSpansOrigin,
) -> IndexSignatureSpans {
    let (anchor, member_path) = match origin {
        IndexSignatureSpansOrigin::Authored {
            anchor,
            member_path,
        } => (anchor, member_path),
        IndexSignatureSpansOrigin::Synthetic(_) => return IndexSignatureSpans::default(),
    };
    let Some(TSSignature::TSIndexSignature(idx)) =
        resolve_member(program, anchor.contributor_index, member_path)
    else {
        return IndexSignatureSpans::default();
    };
    IndexSignatureSpans {
        declaration: Some(idx.span.into()),
        key: idx.parameters.first().map(|param| param.span.into()),
        value: Some(idx.type_annotation.type_annotation.span().into()),
    }
}

/// Recover the `FunctionSpans` (signature / return-type spans) of the authored
/// function named by `origin`.
#[must_use]
pub(crate) fn recover_function_spans_from_program(
    program: &Program<'_>,
    origin: &FunctionSpansOrigin,
) -> FunctionSpans {
    match function_type_for(program, origin) {
        Some(func) => FunctionSpans {
            signature: Some(func.signature_span),
            return_type: func.return_type_span,
        },
        None => FunctionSpans::default(),
    }
}

/// Recover a `FunctionParam.span` for the parameter selected by `origin`.
#[must_use]
pub(crate) fn recover_function_param_span_from_program(
    program: &Program<'_>,
    origin: &FunctionParamSpanOrigin,
) -> Option<Span> {
    let func = function_type_for(program, &origin.function)?;
    match origin.param {
        FunctionParamSelector::Positional { ordinal } => {
            func.param_spans.get(ordinal as usize).copied()
        }
        FunctionParamSelector::Rest => func.rest_param_span,
    }
}

// ---------------------------------------------------------------------------
// Outer helpers — compose the pure core with the lease-pinned run path.
// ---------------------------------------------------------------------------

/// Recover `MemberSpans` for `origin` against the retained parse for `key`.
#[must_use]
pub(crate) fn recover_member_spans(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    source: &Arc<str>,
    source_type: oxc_span::SourceType,
    origin: &MemberSpansOrigin,
) -> MemberSpans {
    let origin = origin.clone();
    service
        .run(key, source, source_type, move |program| match program {
            Some(program) => recover_member_spans_from_program(program.borrow_dependent(), &origin),
            None => MemberSpans::default(),
        })
        .value
}

/// Recover `IndexSignatureSpans` for `origin` against the retained parse for
/// `key`.
#[must_use]
pub(crate) fn recover_index_signature_spans(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    source: &Arc<str>,
    source_type: oxc_span::SourceType,
    origin: &IndexSignatureSpansOrigin,
) -> IndexSignatureSpans {
    let origin = origin.clone();
    service
        .run(key, source, source_type, move |program| match program {
            Some(program) => {
                recover_index_signature_spans_from_program(program.borrow_dependent(), &origin)
            }
            None => IndexSignatureSpans::default(),
        })
        .value
}

/// Recover `FunctionSpans` for `origin` against the retained parse for `key`.
#[must_use]
pub(crate) fn recover_function_spans(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    source: &Arc<str>,
    source_type: oxc_span::SourceType,
    origin: &FunctionSpansOrigin,
) -> FunctionSpans {
    let origin = origin.clone();
    service
        .run(key, source, source_type, move |program| match program {
            Some(program) => {
                recover_function_spans_from_program(program.borrow_dependent(), &origin)
            }
            None => FunctionSpans::default(),
        })
        .value
}

/// Recover a `FunctionParam.span` for `origin` against the retained parse for
/// `key`.
#[must_use]
pub(crate) fn recover_function_param_span(
    service: &Arc<DeclLoweringService>,
    key: &SnapshotKey,
    source: &Arc<str>,
    source_type: oxc_span::SourceType,
    origin: &FunctionParamSpanOrigin,
) -> Option<Span> {
    let origin = origin.clone();
    service
        .run(key, source, source_type, move |program| match program {
            Some(program) => {
                recover_function_param_span_from_program(program.borrow_dependent(), &origin)
            }
            None => None,
        })
        .value
}

// ---------------------------------------------------------------------------
// Navigation internals (pure; borrow the retained `&Program`).
// ---------------------------------------------------------------------------

/// The span facts of a located function-like node, converted to `verter_span`.
struct FunctionSpanFacts {
    signature_span: Span,
    return_type_span: Option<Span>,
    param_spans: Vec<Span>,
    rest_param_span: Option<Span>,
}

/// Resolve a member path from a decl body to the target `&TSSignature`. Each
/// path ordinal selects a member of the current object/interface surface; a
/// non-final ordinal descends into that member's value-type surface (a nested
/// type literal). The final ordinal names the target member.
fn resolve_member<'a>(
    program: &'a Program<'a>,
    contributor_index: u32,
    member_path: &[u32],
) -> Option<&'a TSSignature<'a>> {
    let stmt = program.body.get(contributor_index as usize)?;
    let mut members = statement_member_surface(stmt)?;
    let (last, prefix) = member_path.split_last()?;
    for &ordinal in prefix {
        let sig = members.get(ordinal as usize)?;
        members = member_value_surface(sig)?;
    }
    members.get(*last as usize)
}

/// The member surface (`&[TSSignature]`) of a top-level statement, if it is a
/// type alias whose body is a type literal, or an interface declaration.
fn statement_member_surface<'a>(stmt: &'a Statement<'a>) -> Option<&'a [TSSignature<'a>]> {
    match stmt {
        Statement::TSTypeAliasDeclaration(alias) => tstype_member_surface(&alias.type_annotation),
        Statement::TSInterfaceDeclaration(iface) => Some(&iface.body.body),
        _ => None,
    }
}

/// The member surface of a `TSType`, if it is a type literal.
fn tstype_member_surface<'a>(ty: &'a TSType<'a>) -> Option<&'a [TSSignature<'a>]> {
    match ty {
        TSType::TSTypeLiteral(literal) => Some(&literal.members),
        _ => None,
    }
}

/// The member surface reached by descending into a member's value type (a nested
/// type literal), if applicable.
fn member_value_surface<'a>(sig: &'a TSSignature<'a>) -> Option<&'a [TSSignature<'a>]> {
    match sig {
        TSSignature::TSPropertySignature(prop) => {
            let ta = prop.type_annotation.as_ref()?;
            tstype_member_surface(&ta.type_annotation)
        }
        _ => None,
    }
}

/// Extract the span facts of the function-like node an origin names.
fn function_type_for(
    program: &Program<'_>,
    origin: &FunctionSpansOrigin,
) -> Option<FunctionSpanFacts> {
    match origin {
        FunctionSpansOrigin::AliasBody { anchor } => {
            let stmt = program.body.get(anchor.contributor_index as usize)?;
            let Statement::TSTypeAliasDeclaration(alias) = stmt else {
                return None;
            };
            let TSType::TSFunctionType(func) = &alias.type_annotation else {
                return None;
            };
            Some(FunctionSpanFacts {
                signature_span: func.span.into(),
                return_type_span: Some(func.return_type.type_annotation.span().into()),
                param_spans: func.params.items.iter().map(|p| p.span.into()).collect(),
                rest_param_span: func.params.rest.as_ref().map(|r| r.span.into()),
            })
        }
        FunctionSpansOrigin::Member {
            anchor,
            member_path,
        } => {
            let sig = resolve_member(program, anchor.contributor_index, member_path)?;
            signature_function_facts(sig)
        }
        FunctionSpansOrigin::Synthetic(_) => None,
    }
}

/// The function span facts of a method / call / construct member signature.
fn signature_function_facts(sig: &TSSignature<'_>) -> Option<FunctionSpanFacts> {
    match sig {
        TSSignature::TSMethodSignature(method) => Some(FunctionSpanFacts {
            signature_span: method.span.into(),
            return_type_span: method
                .return_type
                .as_ref()
                .map(|rt| rt.type_annotation.span().into()),
            param_spans: method.params.items.iter().map(|p| p.span.into()).collect(),
            rest_param_span: method.params.rest.as_ref().map(|r| r.span.into()),
        }),
        TSSignature::TSCallSignatureDeclaration(call) => Some(FunctionSpanFacts {
            signature_span: call.span.into(),
            return_type_span: call
                .return_type
                .as_ref()
                .map(|rt| rt.type_annotation.span().into()),
            param_spans: call.params.items.iter().map(|p| p.span.into()).collect(),
            rest_param_span: call.params.rest.as_ref().map(|r| r.span.into()),
        }),
        TSSignature::TSConstructSignatureDeclaration(ctor) => Some(FunctionSpanFacts {
            signature_span: ctor.span.into(),
            return_type_span: ctor
                .return_type
                .as_ref()
                .map(|rt| rt.type_annotation.span().into()),
            param_spans: ctor.params.items.iter().map(|p| p.span.into()).collect(),
            rest_param_span: ctor.params.rest.as_ref().map(|r| r.span.into()),
        }),
        _ => None,
    }
}
